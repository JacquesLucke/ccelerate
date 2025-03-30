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
use config::ConfigManager;
use parallel_pool::ParallelPool;
use parking_lot::Mutex;
use parse_gcc::GCCArgs;
use path_utils::make_absolute;
use ratatui::widgets::TableState;
use state::State;
use task_periods::TaskPeriods;

mod ar_args;
mod config;
mod database;
mod parallel_pool;
mod parse_gcc;
mod path_utils;
mod request_ar;
mod request_gcc_eager;
mod request_gcc_final_link;
mod request_gcc_without_link;
mod state;
mod task_log;
mod task_periods;
mod tui;

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
    #[arg(long)]
    log_files: bool,
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
            let config = match state.config_manager.config_for_paths(
                &request_gcc_args
                    .sources
                    .iter()
                    .map(|s| &s.path)
                    .collect::<Vec<_>>(),
            ) {
                Ok(config) => config,
                Err(e) => {
                    return HttpResponse::BadRequest()
                        .body(format!("Error reading config file: {}", e));
                }
            };
            if is_gcc_cmakescratch(&request_gcc_args, &request.cwd)
                || is_gcc_compiler_id_check(&request_gcc_args, &request.cwd)
                || request_gcc_args.primary_output.is_none()
                || request_gcc_args
                    .sources
                    .iter()
                    .any(|p| config.is_eager_path(&p.path))
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
                    &config,
                )
                .await;
            }
            return request_gcc_final_link::handle_gcc_final_link_request(
                request.binary,
                &request_gcc_args,
                &request.cwd,
                state,
                &config,
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
    handle_request(&run_request, &state).await
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

    let cwd = std::env::current_dir()?;
    let data_dir = make_absolute(
        &cwd,
        &cli.data_dir
            .clone()
            .unwrap_or_else(|| PathBuf::from("./ccelerate_data")),
    );
    let db_path = data_dir.join("ccelerate.db");
    let conn = database::load_or_create_db(&db_path)?;
    let addr = format!("127.0.0.1:{}", cli.port);
    let state = actix_web::web::Data::new(State {
        address: addr.clone(),
        conn: Arc::new(Mutex::new(conn)),
        task_periods: TaskPeriods::new(),
        tasks_table_state: Arc::new(Mutex::new(TableState::default())),
        auto_scroll: Arc::new(Mutex::new(true)),
        pool: ParallelPool::new(cli.jobs.unwrap_or_else(|| {
            std::thread::available_parallelism()
                .unwrap_or(NonZeroUsize::new(1).unwrap())
                .get()
        })),
        cli,
        data_dir,
        config_manager: ConfigManager::new(),
    });

    if state.cli.no_tui {
        log::set_logger(&NoTuiLogger {})
            .map(|()| log::set_max_level(log::LevelFilter::Info))
            .unwrap();
        log::info!("Listening on http://{}", addr);
        server_thread(state.clone()).await;
        return Ok(());
    }
    // Run the server in the background and the tui on the main thread.
    tokio::spawn(server_thread(state.clone()));
    match tui::run_tui(&state) {
        Ok(_) => {}
        Err(e) => {
            log::error!("Error running tui: {e}");
        }
    };
    Ok(())
}
