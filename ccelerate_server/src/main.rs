use std::{
    collections::HashMap,
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
use parse_gcc::GCCArgs;
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
mod request_gcc_eager;
mod request_gcc_final_link;
mod request_gcc_without_link;

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
    #[arg(long)]
    data_dir: Option<PathBuf>,
}

struct State {
    address: String,
    conn: Arc<Mutex<rusqlite::Connection>>,
    tasks_logger: TasksLogger,
    tasks_table_state: Arc<Mutex<TableState>>,
    pool: ParallelPool,
    cli: CLI,
    data_dir: PathBuf,
    header_type_cache: Arc<Mutex<HashMap<PathBuf, HeaderType>>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HeaderType {
    Local,
    Global,
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
    data: DbFilesRowData,
}

#[derive(Debug)]
struct DbFilesRowData {
    cwd: PathBuf,
    binary: WrappedBinary,
    args: Vec<OsString>,
    local_code_file: Option<PathBuf>,
    headers: Option<Vec<PathBuf>>,
    global_defines: Option<Vec<String>>,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct DbFilesRowDataStorage {
    cwd: OsString,
    binary: WrappedBinary,
    args: Vec<OsString>,
    local_code_file: Option<OsString>,
    headers: Option<Vec<OsString>>,
    global_defines: Option<Vec<String>>,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct DbFilesRowDataDebug {
    cwd: String,
    binary: WrappedBinary,
    args: Vec<String>,
    local_code_file: Option<String>,
    headers: Option<Vec<String>>,
    global_defines: Option<Vec<String>>,
}

impl DbFilesRowDataStorage {
    fn from_data(data: &DbFilesRowData) -> Self {
        Self {
            cwd: data.cwd.clone().into(),
            binary: data.binary,
            args: data.args.clone(),
            local_code_file: data.local_code_file.clone().map(|s| s.into()),
            headers: data
                .headers
                .clone()
                .map(|h| h.iter().map(|s| s.clone().into()).collect()),
            global_defines: data.global_defines.clone(),
        }
    }

    fn to_data(&self) -> DbFilesRowData {
        DbFilesRowData {
            cwd: self.cwd.clone().into(),
            binary: self.binary,
            args: self.args.clone(),
            local_code_file: self.local_code_file.clone().map(|s| s.into()),
            headers: self
                .headers
                .clone()
                .map(|h| h.iter().map(|s| s.clone().into()).collect()),
            global_defines: self.global_defines.clone(),
        }
    }
}

impl DbFilesRowDataDebug {
    fn from_data(data: &DbFilesRowData) -> Self {
        Self {
            cwd: data.cwd.to_string_lossy().to_string(),
            binary: data.binary,
            args: data
                .args
                .iter()
                .map(|s| s.to_string_lossy().to_string())
                .collect(),
            local_code_file: data
                .local_code_file
                .as_ref()
                .map(|s| s.to_string_lossy().to_string()),
            headers: data
                .headers
                .as_ref()
                .map(|h| h.iter().map(|s| s.to_string_lossy().to_string()).collect()),
            global_defines: data.global_defines.clone(),
        }
    }
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
    conn.execute(
        "INSERT OR REPLACE INTO Files (path, data_debug, data) VALUES (?1, ?2, ?3)",
        rusqlite::params![
            row.path.to_string_lossy(),
            serde_json::to_string_pretty(&DbFilesRowDataDebug::from_data(&row.data)).unwrap(),
            serde_json::to_string(&DbFilesRowDataStorage::from_data(&row.data)).unwrap(),
        ],
    )?;
    Ok(())
}

fn load_db_file(conn: &rusqlite::Connection, path: &Path) -> Option<DbFilesRow> {
    conn.query_row(
        "SELECT data FROM Files WHERE path = ?",
        rusqlite::params![path.to_string_lossy().to_string()],
        |row| {
            // TODO: Support OsStr in the database.
            let data = row.get::<usize, String>(0).unwrap();
            Ok(DbFilesRow {
                path: path.to_path_buf(),
                data: serde_json::from_str::<DbFilesRowDataStorage>(&data)
                    .unwrap()
                    .to_data(),
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
                    data: DbFilesRowData {
                        cwd: request.cwd.clone(),
                        binary: request.binary,
                        args: request_ar_args.to_args(),
                        local_code_file: None,
                        headers: None,
                        global_defines: None,
                    },
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
            let eager_paths = vec![
                "/home/jacques/blender/blender/source/blender/imbuf/movie",
                "/home/jacques/blender/blender/source/blender/python/intern/bpy_app_ffmpeg.cc",
                "wayland_dynload",
                "audaspace",
                "quadriflow",
                "lzma",
                "ghost",
                "intern/cycles",
                "xxhash.c",
                "/home/jacques/blender/blender/source/blender/editors/curve/editcurve.cc",
                "/home/jacques/blender/blender/source/blender/blenkernel/intern/curve_decimate.cc",
                "editcurve_paint.cc",
                "curves_draw.cc",
                "grease_pencil_geom.cc",
            ];
            if is_gcc_cmakescratch(&request_gcc_args, &request.cwd)
                || is_gcc_compiler_id_check(&request_gcc_args, &request.cwd)
                || request_gcc_args.primary_output.is_none()
                || eager_paths.iter().any(|p| {
                    request_gcc_args
                        .sources
                        .first()
                        .unwrap()
                        .path
                        .to_str()
                        .unwrap()
                        .contains(p)
                })
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

    let data_dir = cli
        .data_dir
        .clone()
        .unwrap_or_else(|| PathBuf::from("./ccelerate_data"));

    std::fs::create_dir_all(&data_dir).unwrap();

    let db_migrations = Migrations::new(vec![M::up(
        "CREATE TABLE Files(
            path TEXT NOT NULL PRIMARY KEY,
            data TEXT NOT NULL,
            data_debug TEXT NOT NULL
        );",
    )]);

    let db_path = data_dir.join("ccelerate.db");
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
        data_dir: data_dir,
        header_type_cache: Arc::new(Mutex::new(HashMap::new())),
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
