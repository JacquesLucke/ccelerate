use std::{
    collections::HashSet,
    ffi::{OsStr, OsString},
    io::Write,
    num::NonZeroUsize,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};

use actix_web::{HttpResponse, web::Data};
use anyhow::Result;
use ccelerate_shared::{RunRequestData, RunRequestDataWire, WrappedBinary};
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
use tokio::task::JoinHandle;

mod parse_ar;
mod parse_gcc;
mod path_utils;

static ASSETS_DIR: include_dir::Dir = include_dir::include_dir!("$CARGO_MANIFEST_DIR/src/assets");

#[derive(clap::Parser, Debug)]
#[command(name = "ccelerate_server")]
struct CLI {
    #[arg(long, default_value_t = ccelerate_shared::DEFAULT_PORT)]
    port: u16,
    #[arg(long)]
    no_tui: bool,
    #[arg(short, long)]
    jobs: Option<usize>,
}

struct State {
    address: String,
    conn: Arc<Mutex<rusqlite::Connection>>,
    tasks_logger: TasksLogger,
    tasks_table_state: Arc<Mutex<TableState>>,
    pool: ParallelPool,
    cli: CLI,
}

struct TasksLogger {
    tasks: Arc<Mutex<Vec<TaskLog>>>,
}

impl TasksLogger {
    fn new() -> Self {
        Self {
            tasks: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn start_task(&self, name: &str) -> TaskLogHandle {
        let end_time = Arc::new(Mutex::new(None));
        let task = TaskLog {
            name: name.to_string(),
            start_time: Instant::now(),
            end_time: end_time.clone(),
        };
        self.tasks.lock().push(task);
        TaskLogHandle { end_time: end_time }
    }

    fn get_for_print(&self) -> Vec<TaskLogPrint> {
        self.tasks
            .lock()
            .iter()
            .map(|t| TaskLogPrint {
                name: t.name.clone(),
                duration: t.duration(),
                active: t.is_running(),
            })
            .collect()
    }
}

struct TaskLog {
    name: String,
    start_time: Instant,
    end_time: Arc<Mutex<Option<Instant>>>,
}

struct TaskLogPrint {
    name: String,
    duration: Duration,
    active: bool,
}

impl TaskLog {
    fn is_running(&self) -> bool {
        self.end_time.lock().is_none()
    }

    fn duration(&self) -> Duration {
        self.end_time
            .lock()
            .unwrap_or_else(|| Instant::now())
            .duration_since(self.start_time)
    }
}

struct TaskLogHandle {
    end_time: Arc<Mutex<Option<Instant>>>,
}

impl Drop for TaskLogHandle {
    fn drop(&mut self) {
        *self.end_time.lock() = Some(Instant::now());
    }
}

struct DbFilesRow {
    path: PathBuf,
    cwd: PathBuf,
    binary: WrappedBinary,
    args: Vec<OsString>,
}

struct ParallelPool {
    semaphore: Arc<tokio::sync::Semaphore>,
}

impl ParallelPool {
    fn new(num: usize) -> Self {
        Self {
            semaphore: Arc::new(tokio::sync::Semaphore::new(num)),
        }
    }

    fn run<F, Fut>(&self, f: F) -> JoinHandle<()>
    where
        F: FnOnce() -> Fut + Send + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        let permit = self.semaphore.clone().acquire_owned();
        tokio::task::spawn(async move {
            let _permit = permit.await.unwrap();
            f().await;
        })
    }
}

fn store_db_file(conn: &rusqlite::Connection, row: &DbFilesRow) -> rusqlite::Result<()> {
    // TODO: Support OsStr in the database.
    conn.execute(
        "INSERT OR REPLACE INTO Files (binary, path, cwd, args) VALUES (?1, ?2, ?3, ?4)",
        rusqlite::params![
            row.binary.to_standard_binary_name().to_string_lossy(),
            row.path.to_string_lossy(),
            row.cwd.to_string_lossy(),
            serde_json::to_string(
                &row.args
                    .iter()
                    .map(|s| s.to_string_lossy())
                    .collect::<Vec<_>>()
            )
            .unwrap()
        ],
    )?;
    Ok(())
}

fn load_db_file(conn: &rusqlite::Connection, path: &Path) -> Option<DbFilesRow> {
    conn.query_row(
        "SELECT binary, cwd, args FROM Files WHERE path = ?",
        rusqlite::params![path.to_string_lossy().to_string()],
        |row| {
            // TODO: Support OsStr in the database.
            let binary = row.get::<usize, String>(0).unwrap();
            let cwd = row.get::<usize, String>(1).unwrap();
            let args = row.get::<usize, String>(2).unwrap();
            Ok(DbFilesRow {
                path: path.to_path_buf(),
                cwd: Path::new(&cwd).to_path_buf(),
                binary: WrappedBinary::from_standard_binary_name(OsStr::new(&binary)).unwrap(),
                args: serde_json::from_str::<Vec<String>>(&args)
                    .unwrap()
                    .into_iter()
                    .map(OsString::from)
                    .collect(),
            })
        },
    )
    .ok()
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

fn find_smallest_link_units(
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
                let file_row = load_db_file(conn, &current_path);
                if let Some(file_row) = file_row {
                    match file_row.binary {
                        binary if binary.is_gcc_compatible() => {
                            let args = GCCArgs::parse_owned(&file_row.cwd, file_row.args).unwrap();
                            remaining_paths.extend(args.sources.iter().map(|s| s.path.clone()));
                        }
                        binary if binary.is_ar_compatible() => {
                            let args = ArArgs::parse_owned(&file_row.cwd, file_row.args).unwrap();
                            remaining_paths.extend(args.sources.iter().map(|s| s.clone()));
                        }
                        binary => {
                            panic!("Cannot handle binary: {:?}", binary);
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

async fn handle_eager_gcc_request(
    binary: WrappedBinary,
    request_gcc_args: &GCCArgs,
    cwd: &Path,
    state: &State,
) -> HttpResponse {
    let _log_handle = state.tasks_logger.start_task(&format!(
        "Eager: {:?} {}",
        binary.to_standard_binary_name(),
        request_gcc_args
            .to_args()
            .iter()
            .map(|s| s.to_string_lossy())
            .collect::<Vec<_>>()
            .join(" ")
    ));
    let child = tokio::process::Command::new(binary.to_standard_binary_name())
        .args(request_gcc_args.to_args())
        .current_dir(&cwd)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn();
    let Ok(child) = child else {
        return HttpResponse::InternalServerError().body("Failed to spawn child");
    };
    let Ok(child_result) = child.wait_with_output().await else {
        return HttpResponse::InternalServerError().body("Failed to wait on child");
    };
    HttpResponse::Ok().json(
        &ccelerate_shared::RunResponseData {
            stdout: child_result.stdout,
            stderr: child_result.stderr,
            status: child_result.status.code().unwrap_or(1),
        }
        .to_wire(),
    )
}

async fn handle_gcc_without_link_request(
    binary: WrappedBinary,
    request_gcc_args: &GCCArgs,
    cwd: &Path,
    state: &State,
) -> HttpResponse {
    let Some(request_output_path) = request_gcc_args.primary_output.as_ref() else {
        return HttpResponse::NotImplemented().body("Expected output path");
    };

    let _log_handle = state.tasks_logger.start_task(&format!(
        "Prepare: {:?}",
        request_output_path.file_name().unwrap().to_string_lossy()
    ));
    store_db_file(
        &state.conn.lock(),
        &DbFilesRow {
            path: request_output_path.clone(),
            cwd: cwd.to_path_buf(),
            binary: binary,
            args: request_gcc_args.to_args(),
        },
    )
    .unwrap();
    let dummy_object = ASSETS_DIR.get_file("dummy_object.o").unwrap();
    tokio::fs::write(request_output_path, dummy_object.contents())
        .await
        .unwrap();
    HttpResponse::Ok().json(&ccelerate_shared::RunResponseDataWire {
        ..Default::default()
    })
}

#[derive(Debug, Clone)]
struct WrappedLinkUnit {
    original_object_path: PathBuf,
    wrapped_object_path: PathBuf,
}

fn osstring_to_osstr_vec(s: &[OsString]) -> Vec<&OsStr> {
    s.iter().map(|s| s.as_ref()).collect()
}

async fn build_wrapped_link_units(link_units: &[WrappedLinkUnit], state: &Data<State>) {
    let link_units = link_units.to_vec();
    let handles = link_units
        .into_iter()
        .map(|unit| {
            let Some(original_unit_info) =
                load_db_file(&state.conn.lock(), &unit.original_object_path)
            else {
                panic!("There should be information stored about this file");
            };
            let Ok(original_gcc_args) = GCCArgs::parse(
                &original_unit_info.cwd,
                &osstring_to_osstr_vec(&original_unit_info.args),
            ) else {
                panic!("Cannot parse original gcc arguments");
            };
            let state_clone = state.clone();
            state.pool.run(async move || {
                let mut modified_gcc_args = original_gcc_args;
                modified_gcc_args.primary_output = Some(unit.wrapped_object_path.clone());
                modified_gcc_args.depfile_generate = false;
                modified_gcc_args.depfile_target_name = None;
                modified_gcc_args.depfile_output_path = None;

                log::info!("Compile: {:#?}", modified_gcc_args);

                let _log_handle = state_clone.tasks_logger.start_task(&format!(
                    "Compile: {}",
                    unit.wrapped_object_path
                        .file_name()
                        .unwrap()
                        .to_string_lossy()
                ));

                let child = tokio::process::Command::new(
                    original_unit_info.binary.to_standard_binary_name(),
                )
                .args(modified_gcc_args.to_args())
                .current_dir(&original_unit_info.cwd)
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn();
                let Ok(child) = child else {
                    panic!("Failed to spawn child");
                };
                let Ok(_child_result) = child.wait_with_output().await else {
                    panic!("Failed to wait on child");
                };
            })
        })
        .collect::<Vec<_>>();
    for handle in handles {
        handle.await.unwrap();
    }
}

async fn handle_gcc_final_link_request(
    binary: WrappedBinary,
    request_gcc_args: &GCCArgs,
    cwd: &Path,
    state: &Data<State>,
) -> HttpResponse {
    let tmp_dir = tempfile::tempdir().unwrap();
    let Ok(smallest_link_units) = find_smallest_link_units(&request_gcc_args, &state.conn.lock())
    else {
        return HttpResponse::InternalServerError().body("Failed to find link sources");
    };
    let mut wrapped_link_units = Vec::new();
    let mut unmodified_link_units = Vec::new();
    for link_unit in &smallest_link_units {
        if link_unit.extension() == Some(OsStr::new("o")) {
            wrapped_link_units.push(WrappedLinkUnit {
                original_object_path: link_unit.clone(),
                wrapped_object_path: tmp_dir.path().join(format!(
                    "{}_{}",
                    uuid::Uuid::new_v4().to_string(),
                    link_unit.file_name().unwrap().to_string_lossy()
                )),
            });
        } else {
            unmodified_link_units.push(link_unit.clone());
        }
    }
    log::info!("Building wrapped link units: {:#?}", wrapped_link_units);

    build_wrapped_link_units(&wrapped_link_units, state).await;

    let wrapped_units_archive_path = tmp_dir.path().join("wrapped_units.a");
    let wrapped_units_archive_args = ArArgs {
        flag_c: true,
        flag_q: true,
        flag_s: true,
        thin_archive: true,
        output: Some(wrapped_units_archive_path.clone()),
        sources: wrapped_link_units
            .iter()
            .map(|u| u.wrapped_object_path.clone())
            .collect(),
    };
    {
        let _task_handle = state.tasks_logger.start_task(&format!(
            "Build thin archive: {}",
            wrapped_units_archive_path.to_string_lossy()
        ));
        tokio::process::Command::new(WrappedBinary::Ar.to_standard_binary_name())
            .args(wrapped_units_archive_args.to_args())
            .current_dir(&cwd)
            .spawn()
            .unwrap()
            .wait_with_output()
            .await
            .unwrap();
    }

    let mut modified_gcc_args = request_gcc_args.clone();
    modified_gcc_args.sources = vec![];
    modified_gcc_args.sources.push(SourceFile {
        path: wrapped_units_archive_path.clone(),
        language: None,
    });
    modified_gcc_args.sources.extend(
        unmodified_link_units
            .iter()
            .map(|w| SourceFile {
                path: w.clone(),
                language: None,
            })
            .collect::<Vec<_>>(),
    );

    let _link_task_handle = state.tasks_logger.start_task(&format!(
        "Link: {}",
        modified_gcc_args
            .primary_output
            .as_ref()
            .unwrap_or(&PathBuf::from(""))
            .to_string_lossy()
    ));

    modified_gcc_args.use_link_group = true;
    log::info!("Link: {:#?}", modified_gcc_args.to_args());
    let child = tokio::process::Command::new(binary.to_standard_binary_name())
        .args(modified_gcc_args.to_args())
        .current_dir(&cwd)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn();
    let Ok(child) = child else {
        return HttpResponse::InternalServerError().body("Failed to spawn child");
    };
    let Ok(child_result) = child.wait_with_output().await else {
        return HttpResponse::InternalServerError().body("Failed to wait on child");
    };
    HttpResponse::Ok().json(
        &ccelerate_shared::RunResponseData {
            stdout: child_result.stdout,
            stderr: child_result.stderr,
            status: child_result.status.code().unwrap_or(1),
        }
        .to_wire(),
    )
}

async fn handle_request(request: &RunRequestData, state: &Data<State>) -> HttpResponse {
    let request_args_ref: Vec<&OsStr> = request.args.iter().map(|s| s.as_ref()).collect::<Vec<_>>();
    match request.binary {
        WrappedBinary::Ar => {
            let Ok(request_ar_args) = ArArgs::parse(&request.cwd, &request_args_ref) else {
                return HttpResponse::NotImplemented().body("Cannot parse ar arguments");
            };
            let Some(request_output_path) = request_ar_args.output.as_ref() else {
                return HttpResponse::NotImplemented().body("Expected output path");
            };
            let _task_handle = state.tasks_logger.start_task(&format!(
                "Prepare: {}",
                request_output_path.file_name().unwrap().to_string_lossy()
            ));
            store_db_file(
                &state.conn.lock(),
                &DbFilesRow {
                    path: request_output_path.clone(),
                    cwd: request.cwd.clone(),
                    binary: request.binary,
                    args: request_ar_args.to_args(),
                },
            )
            .unwrap();
            let dummy_archive = ASSETS_DIR.get_file("dummy_archive.a").unwrap();
            std::fs::write(request_output_path, dummy_archive.contents()).unwrap();
            return HttpResponse::Ok().json(&ccelerate_shared::RunResponseDataWire {
                ..Default::default()
            });
        }
        WrappedBinary::Gcc | WrappedBinary::Gxx | WrappedBinary::Clang | WrappedBinary::Clangxx => {
            let Ok(request_gcc_args) = GCCArgs::parse(&request.cwd, &request_args_ref) else {
                return HttpResponse::NotImplemented().body("Cannot parse gcc arguments");
            };
            if is_gcc_cmakescratch(&request_gcc_args, &request.cwd)
                || is_gcc_compiler_id_check(&request_gcc_args, &request.cwd)
                || request_gcc_args.primary_output.is_none()
            {
                return handle_eager_gcc_request(
                    request.binary,
                    &request_gcc_args,
                    &request.cwd,
                    state,
                )
                .await;
            }
            if request_gcc_args.stop_before_link {
                return handle_gcc_without_link_request(
                    request.binary,
                    &request_gcc_args,
                    &request.cwd,
                    state,
                )
                .await;
            }
            return handle_gcc_final_link_request(
                request.binary,
                &request_gcc_args,
                &request.cwd,
                state,
            )
            .await;
        }
    };
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
    let cli: CLI = clap::Parser::parse();

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
    conn.pragma_update(None, "journal_mode", "WAL")?;
    db_migrations.to_latest(&mut conn)?;

    let addr = format!("127.0.0.1:{}", cli.port);
    let state = actix_web::web::Data::new(State {
        address: addr.clone(),
        conn: Arc::new(Mutex::new(conn)),
        tasks_logger: TasksLogger::new(),
        tasks_table_state: Arc::new(Mutex::new(TableState::default())),
        pool: ParallelPool::new(cli.jobs.unwrap_or_else(|| {
            std::thread::available_parallelism()
                .unwrap_or(NonZeroUsize::new(1).unwrap())
                .get()
        })),
        cli: cli,
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

    let mut tasks = state.tasks_logger.get_for_print();
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
    let text = ratatui::text::Text::raw(&state.address);
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
