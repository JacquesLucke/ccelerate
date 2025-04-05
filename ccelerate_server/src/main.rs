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
use ccelerate_shared::{RunRequestData, RunRequestDataWire, RunResponseData, WrappedBinary};
use config::ConfigManager;
use os_str_bytes::OsStrBytesExt;
use parallel_pool::ParallelPool;
use parking_lot::Mutex;
use path_utils::make_absolute;
use ratatui::widgets::TableState;
use state::State;
use task_periods::TaskPeriods;

mod ar_args;
mod code_language;
mod config;
mod database;
mod export_trace;
mod gcc_args;
mod local_code;
mod parallel_pool;
mod path_utils;
mod preprocessor_directives;
mod source_file;
mod state;
mod state_persistent;
mod task_periods;
mod tui;
mod wrap_compile_object_file;
mod wrap_create_static_archive;
mod wrap_eager;
mod wrap_final_link;

static ASSETS_DIR: include_dir::Dir = include_dir::include_dir!("$CARGO_MANIFEST_DIR/src/assets");

struct WebState {
    state: Arc<State>,
}

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

fn gcc_args_have_marker<S: AsRef<OsStr>>(args: &[S], marker: &str) -> bool {
    for arg in args {
        if arg.as_ref().contains(marker) {
            return true;
        }
    }
    false
}

fn gcc_args_or_cwd_have_marker<S: AsRef<OsStr>>(args: &[S], cwd: &Path, marker: &str) -> bool {
    if gcc_args_have_marker(args, marker) {
        return true;
    }
    if cwd.as_os_str().contains(marker) {
        return true;
    }
    false
}

fn is_gcc_compiler_id_check<S: AsRef<OsStr>>(args: &[S], cwd: &Path) -> bool {
    gcc_args_or_cwd_have_marker(args, cwd, "CompilerIdC")
}

fn is_gcc_cmakescratch<S: AsRef<OsStr>>(args: &[S], cwd: &Path) -> bool {
    gcc_args_or_cwd_have_marker(args, cwd, "CMakeScratch")
}

#[derive(Debug, Clone)]
pub struct CommandOutput {
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub status: i32,
}

impl std::error::Error for CommandOutput {}

impl std::fmt::Display for CommandOutput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", String::from_utf8_lossy(&self.stdout))?;
        write!(f, "{}", String::from_utf8_lossy(&self.stderr))?;
        write!(f, "Status: {}", self.status)?;
        Ok(())
    }
}

impl CommandOutput {
    pub fn new_ok() -> Self {
        Self {
            stdout: Vec::new(),
            stderr: Vec::new(),
            status: 0,
        }
    }

    pub fn from_result(result: anyhow::Result<CommandOutput>) -> Self {
        match result {
            Ok(output) => output,
            Err(err) => CommandOutput {
                stdout: Vec::new(),
                stderr: format!("{err}").into_bytes(),
                status: 1,
            },
        }
    }

    pub fn from_process_output(child: std::process::Output) -> Self {
        Self {
            stdout: child.stdout,
            stderr: child.stderr,
            status: child.status.code().unwrap_or(1),
        }
    }
}

async fn handle_request(request: &RunRequestData, state: &Arc<State>) -> Result<CommandOutput> {
    match request.binary {
        WrappedBinary::Ar => {
            return wrap_create_static_archive::wrap_create_static_archive(
                request.binary,
                &request.args,
                &request.cwd,
                state,
            )
            .await;
        }
        WrappedBinary::Gcc | WrappedBinary::Gxx | WrappedBinary::Clang | WrappedBinary::Clangxx => {
            let files = gcc_args::BuildFilesInfo::from_args(&request.cwd, &request.args);

            let known_sources = match &files {
                Ok(files) => files.sources.as_slice(),
                Err(_) => &[],
            };
            let mut paths_for_config: Vec<&Path> = vec![request.cwd.as_ref()];
            paths_for_config.extend(known_sources.iter().map(|s| s.path.as_path()));

            let has_output = match &files {
                Ok(files) => files.output.is_some(),
                Err(_) => false,
            };
            let config = state.config_manager.config_for_paths(&paths_for_config)?;
            if is_gcc_cmakescratch(&request.args, &request.cwd)
                || is_gcc_compiler_id_check(&request.args, &request.cwd)
                || !has_output
                || known_sources.iter().any(|p| config.is_eager_path(&p.path))
            {
                return wrap_eager::wrap_eager(request.binary, &request.args, &request.cwd, state)
                    .await;
            }
            match gcc_args::is_build_object_file(&request.args)? {
                true => {
                    wrap_compile_object_file::wrap_compile_object_file(
                        request.binary,
                        &request.args,
                        &request.cwd,
                        state,
                        &config,
                    )
                    .await
                }
                false => {
                    wrap_final_link::wrap_final_link(
                        request.binary,
                        &request.args,
                        &request.cwd,
                        state,
                        &config,
                    )
                    .await
                }
            }
        }
    }
}

#[actix_web::post("/run")]
async fn route_run(
    run_request: actix_web::web::Json<RunRequestDataWire>,
    web_state: Data<WebState>,
) -> impl actix_web::Responder {
    let Ok(run_request) = RunRequestData::from_wire(&run_request) else {
        log::error!("Could not parse: {:#?}", run_request);
        return HttpResponse::InternalServerError().body("Failed to parse request");
    };
    let output = CommandOutput::from_result(handle_request(&run_request, &web_state.state).await);
    HttpResponse::Ok().json(
        RunResponseData {
            stdout: output.stdout,
            stderr: output.stderr,
            status: output.status,
        }
        .to_wire(),
    )
}

async fn server_thread(state: Arc<State>) {
    let web_state = actix_web::web::Data::new(WebState { state });
    let web_state_clone = web_state.clone();
    actix_web::HttpServer::new(move || {
        actix_web::App::new()
            .app_data(web_state.clone())
            .service(route_index)
            .service(route_run)
    })
    .client_request_timeout(Duration::from_secs(0))
    .bind(web_state_clone.state.address.clone())
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
    let addr = format!("127.0.0.1:{}", cli.port);
    let state = Arc::new(State {
        address: addr.clone(),
        persistent_state: state_persistent::PersistentState::new(&db_path)?,
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
    match tui::run_tui(&state).await {
        Ok(_) => {}
        Err(e) => {
            log::error!("Error running tui: {e}");
        }
    };
    Ok(())
}
