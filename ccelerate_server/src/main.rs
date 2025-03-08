use std::{
    collections::HashSet,
    ffi::OsStr,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};

use actix_web::HttpResponse;
use anyhow::Result;
use ccelerate_shared::{RunRequestData, RunRequestDataWire, WrappedBinary};
use command::{Command, CommandArgs};
use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use parking_lot::Mutex;
use parse_ar::ArArgs;
use parse_gcc::{GCCArgs, SourceFile};
use ratatui::{
    layout::Layout,
    style::{Color, Style},
    widgets::TableState,
};
use rusqlite_migration::{M, Migrations};
use tokio::{task::JoinHandle, time::sleep};

mod command;
mod parse_ar;
mod parse_gcc;
mod path_utils;

static ASSETS_DIR: include_dir::Dir = include_dir::include_dir!("$CARGO_MANIFEST_DIR/src/assets");

struct State {
    address: String,
    conn: Arc<Mutex<rusqlite::Connection>>,
    items: Arc<Mutex<Vec<TaskItem>>>,
    items_table_state: Arc<Mutex<TableState>>,
}

struct TaskItem {
    data: command::Command,
    active: Arc<Mutex<bool>>,
    start_time: Instant,
    end_time: Arc<Mutex<Option<Instant>>>,
}

impl TaskItem {
    fn duration(&self) -> f64 {
        self.end_time
            .lock()
            .unwrap_or_else(|| Instant::now())
            .duration_since(self.start_time)
            .as_secs_f64()
    }
}

#[actix_web::get("/")]
async fn route_index() -> impl actix_web::Responder {
    "ccelerator".to_string()
}

fn is_marker_in_args_or_cwd(command: &command::Command, marker: &str) -> bool {
    if command.cwd.to_string_lossy().contains(marker) {
        return true;
    }
    for arg in &command.to_args() {
        if arg.to_string_lossy().contains(marker) {
            return true;
        }
    }
    false
}

fn is_cmake_scratch_command(command: &command::Command) -> bool {
    is_marker_in_args_or_cwd(command, "CMakeScratch")
}

fn is_compiler_id_check_command(command: &command::Command) -> bool {
    is_marker_in_args_or_cwd(command, "CompilerIdC")
}

fn is_print_sysroot_command(command: &command::Command) -> bool {
    let CommandArgs::Gcc(args) = &command.args else {
        return false;
    };
    args.print_sysroot
}

fn get_command_for_file(path: &Path, conn: &rusqlite::Connection) -> Option<command::Command> {
    let command = conn.query_row(
        "SELECT binary, cwd, args FROM Files WHERE path = ?",
        rusqlite::params![path.to_string_lossy().to_string()],
        |row| {
            // TODO: Support OsStr in the database.
            let binary = row.get::<usize, String>(0).unwrap();
            let cwd = row.get::<usize, String>(1).unwrap();
            let args = row.get::<usize, String>(2).unwrap();
            Ok(Command::new(
                WrappedBinary::from_standard_binary_name(OsStr::new(&binary)).unwrap(),
                Path::new(&cwd),
                serde_json::from_str::<Vec<String>>(&args)
                    .unwrap()
                    .iter()
                    .map(|s| OsStr::new(s))
                    .collect::<Vec<_>>()
                    .as_slice(),
            ))
        },
    );
    let Ok(command) = command else {
        return None;
    };
    command.ok()
}

fn find_root_link_sources(
    link_args: &GCCArgs,
    conn: &rusqlite::Connection,
) -> Result<Vec<PathBuf>> {
    let mut final_sources = HashSet::new();
    let mut remaining_paths = vec![];
    for arg in &link_args.sources {
        remaining_paths.push(arg.path.clone());
    }
    while let Some(current_path) = remaining_paths.pop() {
        match current_path.to_string_lossy().to_string() {
            p if p.ends_with(".o") => {
                final_sources.insert(current_path.clone());
            }
            p if p.ends_with(".a") => {
                let file_command = get_command_for_file(&current_path, conn);
                if let Some(file_command) = file_command {
                    match file_command.args {
                        CommandArgs::Gcc(args) => {
                            remaining_paths.extend(args.sources.iter().map(|s| s.path.clone()));
                        }
                        CommandArgs::Ar(args) => {
                            remaining_paths.extend(args.sources.iter().map(|s| s.clone()));
                        }
                    }
                } else {
                    final_sources.insert(current_path.clone());
                }
            }
            p if p.ends_with(".so") || p.contains(".so.") => {
                final_sources.insert(current_path.clone());
            }
            _ => {
                panic!("unhandled extension: {:?}", current_path);
            }
        };
    }
    Ok(final_sources.into_iter().collect())
}

fn is_gcc_compatible_binary(binary: &str) -> bool {
    match binary {
        "gcc" | "g++" | "clang" | "clang++" => true,
        _ => false,
    }
}

fn is_ar_compatible_binary(binary: &str) -> bool {
    match binary {
        "ar" => true,
        _ => false,
    }
}

#[actix_web::post("/run")]
async fn route_run(
    run_request: actix_web::web::Json<RunRequestDataWire>,
    state: actix_web::web::Data<State>,
) -> impl actix_web::Responder {
    let Ok(run_request) = RunRequestData::from_wire(&run_request) else {
        eprintln!("Could not parse: {:#?}", run_request);
        return HttpResponse::InternalServerError().body("Failed to parse request");
    };
    let Ok(command) = command::Command::new(
        run_request.binary,
        &run_request.cwd,
        &run_request
            .args
            .iter()
            .map(|a| OsStr::new(a))
            .collect::<Vec<_>>(),
    ) else {
        eprintln!("Could not parse: {:#?}", run_request);
        std::process::exit(1);
    };

    if let Some(output_path) = command.primary_output_path() {
        let conn = state.conn.lock();
        // TODO: Support OsStr in the database.
        conn.execute(
            "INSERT OR REPLACE INTO Files (binary, path, cwd, args) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![
                run_request
                    .binary
                    .to_standard_binary_name()
                    .to_string_lossy(),
                output_path.to_string_lossy(),
                run_request.cwd.to_str().unwrap(),
                serde_json::to_string(
                    &run_request
                        .args
                        .iter()
                        .map(|s| s.to_string_lossy())
                        .collect::<Vec<_>>()
                )
                .unwrap()
            ],
        )
        .unwrap();
    }
    let end_props = {
        let mut items = state.items.lock();
        items.push(TaskItem {
            data: command.clone(),
            active: Arc::new(Mutex::new(true)),
            start_time: Instant::now(),
            end_time: Arc::new(Mutex::new(None)),
        });
        state.items_table_state.lock().select_last();
        let last_item = items.last().unwrap();
        (last_item.active.clone(), last_item.end_time.clone())
    };
    scopeguard::defer! {
        *end_props.0.lock() = false;
        *end_props.1.lock() = Some(Instant::now());
    }

    match &command.args {
        CommandArgs::Gcc(args) => {
            if is_cmake_scratch_command(&command)
                || is_compiler_id_check_command(&command)
                || is_print_sysroot_command(&command)
            {
                let Ok(result) = command.run() else {
                    return HttpResponse::InternalServerError().body("Failed to spawn command");
                };
                let Ok(result) = result.wait_with_output().await else {
                    return HttpResponse::InternalServerError().body("Failed to wait on command");
                };
                return HttpResponse::Ok().json(
                    &ccelerate_shared::RunResponseData {
                        stdout: result.stdout,
                        stderr: result.stderr,
                        status: result.status.code().unwrap_or(1),
                    }
                    .to_wire(),
                );
            }
            if args.stop_before_link {
                let dummy_object = ASSETS_DIR.get_file("dummy.o").unwrap();
                let object_path = args.primary_output.as_ref().unwrap();
                println!("Preprocess: {:?}", object_path);
                std::fs::write(object_path, dummy_object.contents()).unwrap();
                return HttpResponse::Ok().json(&ccelerate_shared::RunResponseDataWire {
                    ..Default::default()
                });
            }
            // This does the final link.
            let tmp_dir = tempfile::tempdir().unwrap();
            let Ok(sources) = find_root_link_sources(&args, &state.conn.lock()) else {
                return HttpResponse::InternalServerError()
                    .body("Failed to find root link sources");
            };
            let object_file_sources: Vec<_> = sources
                .iter()
                .filter(|s| s.extension() == Some(OsStr::new("o")))
                .collect();
            let other_file_sources: Vec<_> = sources
                .iter()
                .filter(|s| s.extension() != Some(OsStr::new("o")))
                .collect();

            struct ObjectSourceFile {
                object_path: PathBuf,
                handle: JoinHandle<()>,
            }

            let semaphore = Arc::new(tokio::sync::Semaphore::new(20));
            let object_sources: Vec<_> = object_file_sources
                .iter()
                .map(|source| {
                    let source_command = get_command_for_file(&source, &state.conn.lock());
                    let permit = semaphore.clone().acquire_owned();
                    let object_path = tmp_dir.path().join(format!(
                        "{}_{}",
                        uuid::Uuid::new_v4().to_string(),
                        source.file_name().unwrap().to_string_lossy()
                    ));
                    let object_path_clone = object_path.clone();
                    let handle = tokio::task::spawn(async move {
                        let _permit = permit.await.unwrap();
                        if let Some(mut source_command) = source_command {
                            let CommandArgs::Gcc(args) = &mut source_command.args else {
                                panic!("Expected Gcc");
                            };
                            println!("Compile: {:?}", object_path);
                            args.primary_output = Some(object_path.clone());
                            source_command
                                .run()
                                .unwrap()
                                .wait_with_output()
                                .await
                                .unwrap();
                        }
                    });
                    ObjectSourceFile {
                        object_path: object_path_clone,
                        handle,
                    }
                })
                .collect();
            let mut object_source_paths = vec![];
            for object_source in object_sources {
                object_source.handle.await.unwrap();
                object_source_paths.push(object_source.object_path);
            }

            let tmp_lib_path = tmp_dir.path().join("my_tmp_lib.a");
            let thin_archive_args = ArArgs {
                flag_c: true,
                flag_q: true,
                flag_s: true,
                flag_t: true,
                output: Some(tmp_lib_path.clone().into()),
                sources: object_source_paths.iter().map(|s| s.clone()).collect(),
                ..Default::default()
            };
            Command {
                binary: WrappedBinary::Ar,
                cwd: run_request.cwd.clone(),
                args: CommandArgs::Ar(thin_archive_args),
            }
            .run()
            .unwrap()
            .wait_with_output()
            .await
            .unwrap();

            let mut updated_args = args.clone();
            updated_args.sources = other_file_sources
                .iter()
                .map(|s| SourceFile {
                    path: s.to_path_buf(),
                    language: None,
                })
                .collect();
            updated_args.sources.insert(
                0,
                SourceFile {
                    path: tmp_lib_path.into(),
                    language: None,
                },
            );
            updated_args.use_groups = true;
            let updated_command = Command {
                binary: run_request.binary.clone(),
                cwd: run_request.cwd.clone(),
                args: CommandArgs::Gcc(updated_args),
            };
            println!("Link: {:#?}", updated_command);
            let result = updated_command
                .run()
                .unwrap()
                .wait_with_output()
                .await
                .unwrap();
            return HttpResponse::Ok().json(
                &ccelerate_shared::RunResponseData {
                    stdout: result.stdout,
                    stderr: result.stderr,
                    status: result.status.code().unwrap_or(1),
                }
                .to_wire(),
            );
        }
        CommandArgs::Ar(args) => {
            let dummy_archive = ASSETS_DIR.get_file("dummy_lib.a").unwrap();
            let archive_path = args.output.as_ref().unwrap();
            std::fs::write(archive_path, dummy_archive.contents()).unwrap();
            return HttpResponse::Ok().json(&ccelerate_shared::RunResponseDataWire {
                ..Default::default()
            });
        }
    };
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

    sleep(Duration::from_secs(1000)).await;

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
    items.sort_by_key(|item| {
        let active = *item.active.lock();
        if active {
            (active, (item.duration() * 100f64) as u64)
        } else {
            (active, 0)
        }
    });
    let mut items_table_state = state.items_table_state.lock();

    let vertical = Layout::vertical([Length(1), Min(0)]);
    let [title_area, main_area] = vertical.areas(frame.area());
    let text = ratatui::text::Text::raw(&state.address);
    frame.render_widget(text, title_area);

    let done_style = Style::new().fg(Color::Green);
    let not_done_style = Style::new().fg(Color::Blue);

    let table = ratatui::widgets::Table::new(
        items.iter().map(|item| {
            ratatui::widgets::Row::new([
                ratatui::text::Text::raw(format!(
                    "{:3.1}s",
                    item.end_time
                        .lock()
                        .unwrap_or_else(|| Instant::now())
                        .duration_since(item.start_time)
                        .as_secs_f64()
                )),
                ratatui::text::Text::raw(
                    item.data
                        .primary_output_path()
                        .unwrap_or_default()
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_string(),
                ),
            ])
            .style(if *item.active.lock() {
                not_done_style
            } else {
                done_style
            })
        }),
        [Length(10), Percentage(100)],
    );

    frame.render_stateful_widget(table, main_area, &mut items_table_state);
}
