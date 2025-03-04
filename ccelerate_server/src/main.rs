use std::{ffi::OsStr, sync::Arc};

use actix_web::HttpResponse;
use anyhow::Result;
use base64::prelude::*;
use ccelerate_shared::RunRequestData;
use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use parking_lot::Mutex;
use ratatui::{
    layout::Layout,
    style::{Color, Style},
    widgets::TableState,
};
use rusqlite_migration::{M, Migrations};

mod command;
mod parse_ar;
mod parse_gcc;
mod path_utils;

struct State {
    address: String,
    conn: Arc<Mutex<rusqlite::Connection>>,
    items: Arc<Mutex<Vec<TaskItem>>>,
    items_table_state: Arc<Mutex<TableState>>,
}

struct TaskItem {
    data: command::Command,
    active: Arc<Mutex<bool>>,
}

#[actix_web::get("/")]
async fn route_index() -> impl actix_web::Responder {
    "ccelerator".to_string()
}

fn need_eager_evaluation(run_request: &RunRequestData) -> bool {
    let marker = "CMakeScratch";
    if run_request.cwd.to_str().unwrap_or("").contains(marker) {
        return true;
    }
    for arg in &run_request.args {
        if arg.contains(marker) {
            return true;
        }
    }
    return false;
}

#[actix_web::post("/run")]
async fn route_run(
    run_request: actix_web::web::Json<RunRequestData>,
    state: actix_web::web::Data<State>,
) -> impl actix_web::Responder {
    let eager_evaluation = need_eager_evaluation(&run_request);
    let Ok(command) = command::Command::new(
        &run_request.binary,
        &run_request.cwd,
        &run_request
            .args
            .iter()
            .map(|a| OsStr::new(a))
            .collect::<Vec<_>>(),
    ) else {
        eprintln!("Failed: {:#?}", run_request);
        std::process::exit(1);
    };

    if let Some(output_path) = command.primary_output_path() {
        let conn = state.conn.lock();
        conn.execute(
            "INSERT OR REPLACE INTO Files (binary, path, cwd, args) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![
                run_request.binary,
                output_path.to_string_lossy(),
                run_request.cwd.to_str().unwrap(),
                serde_json::to_string(&run_request.args).unwrap()
            ],
        )
        .unwrap();
    }
    let active = {
        let mut items = state.items.lock();
        items.push(TaskItem {
            data: command.clone(),
            active: Arc::new(Mutex::new(true)),
        });
        state.items_table_state.lock().select_last();
        items.last().unwrap().active.clone()
    };
    scopeguard::defer! {
        *active.lock() = false;
    }

    let Ok(child) = command.run() else {
        return HttpResponse::InternalServerError().body("Failed to spawn command");
    };
    let result = child.wait_with_output().await;
    let result = match result {
        Ok(result) => result,
        Err(err) => {
            return HttpResponse::InternalServerError().body(format!("{}", err));
        }
    };
    let response_data = ccelerate_shared::RunResponseData {
        stdout: if eager_evaluation {
            BASE64_STANDARD.encode(&result.stdout)
        } else {
            "".to_string()
        },
        stderr: if eager_evaluation {
            BASE64_STANDARD.encode(&result.stderr)
        } else {
            "".to_string()
        },
        status: result.status.code().unwrap_or(1),
    };
    HttpResponse::Ok().json(&response_data)
}

async fn server_thread(state: actix_web::web::Data<State>) {
    let state_clone = state.clone();
    actix_web::HttpServer::new(move || {
        actix_web::App::new()
            .app_data(state.clone())
            .service(route_index)
            .service(route_run)
    })
    .bind(state_clone.address.clone())
    .unwrap()
    .run()
    .await
    .unwrap();
}

#[tokio::main]
async fn main() -> Result<()> {
    let addr = format!("127.0.0.1:{}", ccelerate_shared::DEFAULT_PORT);
    println!("Listening on http://{}", addr);

    let db_migrations = Migrations::new(vec![M::up(
        "CREATE TABLE Files(
                path TEXT NOT NULL PRIMARY KEY,
                cwd TEXT NOT NULL,
                binary TEXT NOT NULL,
                args JSON NOT NULL
            );",
    )]);

    let db_path = "./ccelerate.db";
    let mut conn = rusqlite::Connection::open(db_path)?;
    db_migrations.to_latest(&mut conn)?;

    let state = actix_web::web::Data::new(State {
        address: addr,
        conn: Arc::new(Mutex::new(conn)),
        items: Arc::new(Mutex::new(Vec::new())),
        items_table_state: Arc::new(Mutex::new(TableState::default())),
    });

    tokio::spawn(server_thread(state.clone()));

    let mut terminal = ratatui::init();

    loop {
        let state_clone = state.clone();
        terminal
            .draw(|frame| {
                draw_terminal(frame, state_clone);
            })
            .expect("failed to draw terminal");
        if crossterm::event::poll(std::time::Duration::from_millis(100)).unwrap() {
            match crossterm::event::read().unwrap() {
                Event::Key(KeyEvent {
                    code: KeyCode::Char('q'),
                    ..
                })
                | Event::Key(KeyEvent {
                    code: KeyCode::Esc, ..
                })
                | Event::Key(KeyEvent {
                    code: KeyCode::Char('c'),
                    modifiers: KeyModifiers::CONTROL,
                    ..
                }) => {
                    break;
                }
                _ => {}
            }
        }
    }
    ratatui::restore();

    Ok(())
}

fn draw_terminal(frame: &mut ratatui::Frame, state: actix_web::web::Data<State>) {
    use ratatui::layout::Constraint::*;

    let mut items = state.items.lock();
    items.sort_by_key(|item| *item.active.lock());
    let mut items_table_state = state.items_table_state.lock();

    let vertical = Layout::vertical([Length(1), Min(0)]);
    let [title_area, main_area] = vertical.areas(frame.area());
    let text = ratatui::text::Text::raw(&state.address);
    frame.render_widget(text, title_area);

    let done_style = Style::new().fg(Color::Green);
    let not_done_style = Style::new().fg(Color::Blue);

    let table = ratatui::widgets::Table::new(
        items.iter().map(|item| {
            ratatui::widgets::Row::new([ratatui::text::Text::raw(
                item.data
                    .primary_output_path()
                    .unwrap_or_default()
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string(),
            )])
            .style(if *item.active.lock() {
                not_done_style
            } else {
                done_style
            })
        }),
        [Percentage(100)],
    );

    frame.render_stateful_widget(table, main_area, &mut items_table_state);
}
