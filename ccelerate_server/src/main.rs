use std::{
    ffi::OsStr,
    io::Write,
    num::NonZeroUsize,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use actix_web::{HttpResponse, web::Data};
use anyhow::Result;
use ccelerate_shared::{RunRequestData, RunRequestDataWire, WrappedBinary};
use config::Config;
use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use parallel_pool::ParallelPool;
use parking_lot::Mutex;
use parse_gcc::GCCArgs;
use ratatui::{
    layout::Layout,
    style::{Color, Style},
    widgets::TableState,
};
use rusqlite_migration::{M, Migrations};
use task_periods::TaskPeriods;

mod config;
mod database;
mod parallel_pool;
mod parse_ar;
mod parse_gcc;
mod path_utils;
mod request_ar;
mod request_gcc_eager;
mod request_gcc_final_link;
mod request_gcc_without_link;
mod task_periods;

static ASSETS_DIR: include_dir::Dir = include_dir::include_dir!("$CARGO_MANIFEST_DIR/src/assets");

#[derive(clap::Parser, Debug)]
#[command(name = "ccelerate_server")]
struct Cli {
    #[arg(long, default_value_t = ccelerate_shared::DEFAULT_PORT)]
    port: u16,
    #[arg(long)]
    no_tui: bool,
    #[arg(short, long)]
    jobs: Option<usize>,
    #[arg(long)]
    data_dir: Option<PathBuf>,
}

struct State {
    address: String,
    conn: Arc<Mutex<rusqlite::Connection>>,
    task_periods: TaskPeriods,
    tasks_table_state: Arc<Mutex<TableState>>,
    pool: ParallelPool,
    cli: Cli,
    data_dir: PathBuf,
    config: Arc<Mutex<Config>>,
}

#[actix_web::get("/")]
async fn route_index() -> impl actix_web::Responder {
    "ccelerator".to_string()
}

fn gcc_args_have_marker(args: &GCCArgs, marker: &str) -> bool {
    for arg in args.to_args() {
        if arg.to_string_lossy().contains(marker) {
            return true;
        }
    }
    false
}

fn gcc_args_or_cwd_have_marker(args: &GCCArgs, cwd: &Path, marker: &str) -> bool {
    if gcc_args_have_marker(args, marker) {
        return true;
    }
    if cwd.to_string_lossy().contains(marker) {
        return true;
    }
    false
}

fn is_gcc_compiler_id_check(args: &GCCArgs, cwd: &Path) -> bool {
    gcc_args_or_cwd_have_marker(args, cwd, "CompilerIdC")
}

fn is_gcc_cmakescratch(args: &GCCArgs, cwd: &Path) -> bool {
    gcc_args_or_cwd_have_marker(args, cwd, "CMakeScratch")
}

async fn handle_request(request: &RunRequestData, state: &Data<State>) -> HttpResponse {
    let request_args_ref: Vec<&OsStr> = request.args.iter().map(|s| s.as_ref()).collect::<Vec<_>>();
    match request.binary {
        WrappedBinary::Ar => {
            return request_ar::handle_ar_request(request, state).await;
        }
        WrappedBinary::Gcc | WrappedBinary::Gxx | WrappedBinary::Clang | WrappedBinary::Clangxx => {
            let Ok(request_gcc_args) = GCCArgs::parse(&request.cwd, &request_args_ref) else {
                return HttpResponse::NotImplemented().body("Cannot parse gcc arguments");
            };
            {
                let mut config = state.config.lock();
                for source in request_gcc_args.sources.iter() {
                    match config.ensure_configs(&source.path) {
                        Ok(_) => {}
                        Err(e) => {
                            return HttpResponse::BadRequest()
                                .body(format!("Error reading config file: {}", e));
                        }
                    }
                }
            }
            if is_gcc_cmakescratch(&request_gcc_args, &request.cwd)
                || is_gcc_compiler_id_check(&request_gcc_args, &request.cwd)
                || request_gcc_args.primary_output.is_none()
                || request_gcc_args
                    .sources
                    .iter()
                    .any(|p| state.config.lock().is_eager_path(&p.path))
            {
                return request_gcc_eager::handle_eager_gcc_request(
                    request.binary,
                    &request_gcc_args,
                    &request.cwd,
                    state,
                )
                .await;
            }
            if request_gcc_args.stop_before_link {
                return request_gcc_without_link::handle_gcc_without_link_request(
                    request.binary,
                    &request_gcc_args,
                    &request.cwd,
                    state,
                )
                .await;
            }
            return request_gcc_final_link::handle_gcc_final_link_request(
                request.binary,
                &request_gcc_args,
                &request.cwd,
                state,
            )
            .await;
        }
    };
}

async fn log_file(state: &Data<State>, name: &str, data: &[u8], ext: &str) -> Result<()> {
    let data_hash = twox_hash::XxHash64::oneshot(0, data);
    let data_hash_str = format!("{:x}", data_hash);
    let file_name = format!("{}.{}", data_hash_str, ext);
    let file_dir = state.data_dir.join("log_files").join(&data_hash_str[..2]);
    let file_path = file_dir.join(file_name);
    tokio::fs::create_dir_all(file_dir).await?;
    tokio::fs::write(&file_path, data).await?;

    let time: chrono::DateTime<chrono::Utc> = chrono::Utc::now();
    state.conn.lock().execute(
        "INSERT OR REPLACE INTO LogFiles (name, path, time) VALUES (?1, ?2, ?3)",
        rusqlite::params![name, file_path.to_string_lossy(), time.to_rfc3339()],
    )?;

    Ok(())
}

#[actix_web::post("/run")]
async fn route_run(
    run_request: actix_web::web::Json<RunRequestDataWire>,
    state: Data<State>,
) -> impl actix_web::Responder {
    let Ok(run_request) = RunRequestData::from_wire(&run_request) else {
        log::error!("Could not parse: {:#?}", run_request);
        return HttpResponse::InternalServerError().body("Failed to parse request");
    };
    return handle_request(&run_request, &state).await;
}

async fn server_thread(state: Data<State>) {
    let state_clone = state.clone();
    actix_web::HttpServer::new(move || {
        actix_web::App::new()
            .app_data(state.clone())
            .service(route_index)
            .service(route_run)
    })
    .client_request_timeout(Duration::from_secs(0))
    .bind(state_clone.address.clone())
    .unwrap()
    .run()
    .await
    .unwrap();
}

struct NoTuiLogger {}

impl log::Log for NoTuiLogger {
    fn enabled(&self, _metadata: &log::Metadata) -> bool {
        true
    }

    fn log(&self, record: &log::Record) {
        println!("{} - {}", record.level(), record.args());
    }

    fn flush(&self) {
        let _ = std::io::stdout().flush();
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli: Cli = clap::Parser::parse();

    let data_dir = cli
        .data_dir
        .clone()
        .unwrap_or_else(|| PathBuf::from("./ccelerate_data"));

    std::fs::create_dir_all(&data_dir).unwrap();

    let db_migrations = Migrations::new(vec![M::up(
        "
        CREATE TABLE Files(
            path TEXT NOT NULL PRIMARY KEY,
            data TEXT NOT NULL,
            data_debug TEXT NOT NULL
        );
        CREATE TABLE LogFiles(
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT NOT NULL,
            path TEXT NOT NULL,
            time TEXT NOT NULL
        );
        ",
    )]);

    let db_path = data_dir.join("ccelerate.db");
    let mut conn = rusqlite::Connection::open(db_path)?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    db_migrations.to_latest(&mut conn)?;

    let addr = format!("127.0.0.1:{}", cli.port);
    let state = actix_web::web::Data::new(State {
        address: addr.clone(),
        conn: Arc::new(Mutex::new(conn)),
        task_periods: TaskPeriods::new(),
        tasks_table_state: Arc::new(Mutex::new(TableState::default())),
        pool: ParallelPool::new(cli.jobs.unwrap_or_else(|| {
            std::thread::available_parallelism()
                .unwrap_or(NonZeroUsize::new(1).unwrap())
                .get()
        })),
        cli,
        data_dir,
        config: Arc::new(Mutex::new(Config::default())),
    });

    if state.cli.no_tui {
        log::set_logger(&NoTuiLogger {})
            .map(|()| log::set_max_level(log::LevelFilter::Info))
            .unwrap();
        log::info!("Listening on http://{}", addr);
        server_thread(state.clone()).await;
        return Ok(());
    }
    tokio::spawn(server_thread(state.clone()));

    let mut terminal = ratatui::init();

    loop {
        let state_clone = state.clone();
        state_clone.tasks_table_state.lock().select_last();
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

    let mut tasks = state.task_periods.get_periods();
    tasks.sort_by_key(|t| {
        (
            t.active,
            if t.active {
                (t.duration.as_secs_f64() * 100f64) as u64
            } else {
                0
            },
        )
    });

    let mut tasks_table_state = state.tasks_table_state.lock();

    let vertical = Layout::vertical([Length(1), Min(0)]);
    let [title_area, main_area] = vertical.areas(frame.area());
    let text = ratatui::text::Text::raw(format!("ccelerate_server at http://{}", state.address));
    frame.render_widget(text, title_area);

    let done_style = Style::new().fg(Color::Green);
    let not_done_style = Style::new().fg(Color::Blue);

    let table = ratatui::widgets::Table::new(
        tasks.iter().map(|t| {
            ratatui::widgets::Row::new([
                ratatui::text::Text::raw(format!("{:3.1}s", t.duration.as_secs_f64())),
                ratatui::text::Text::raw(&t.name),
            ])
            .style(if t.active { not_done_style } else { done_style })
        }),
        [Length(10), Percentage(100)],
    );

    frame.render_stateful_widget(table, main_area, &mut tasks_table_state);
}
