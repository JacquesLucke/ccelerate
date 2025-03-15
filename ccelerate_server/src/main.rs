use std::{
    collections::{HashMap, HashSet},
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
use tokio::{io::AsyncWriteExt, task::JoinHandle};

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
    cwd: PathBuf,
    binary: WrappedBinary,
    args: Vec<OsString>,
    local_code_file: Option<PathBuf>,
    headers: Option<Vec<PathBuf>>,
    global_defines: Option<Vec<String>>,
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
        "INSERT OR REPLACE INTO Files (binary, path, cwd, args, local_code_file, headers, global_defines) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
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
            .unwrap(),
            row.local_code_file.as_ref().map(|s| s.to_string_lossy()),
            row.headers
                .as_ref()
                .map(|h| serde_json::to_string(h).unwrap()),
            row.global_defines
                .as_ref()
                .map(|h| serde_json::to_string(h).unwrap()),
        ],
    )?;
    Ok(())
}

fn load_db_file(conn: &rusqlite::Connection, path: &Path) -> Option<DbFilesRow> {
    conn.query_row(
        "SELECT binary, cwd, args, local_code_file, headers, global_defines FROM Files WHERE path = ?",
        rusqlite::params![path.to_string_lossy().to_string()],
        |row| {
            // TODO: Support OsStr in the database.
            let binary = row.get::<usize, String>(0).unwrap();
            let cwd = row.get::<usize, String>(1).unwrap();
            let args = row.get::<usize, String>(2).unwrap();
            let local_code_file = row.get::<usize, Option<String>>(3).unwrap();
            let headers = row.get::<usize, Option<String>>(4).unwrap();
            let global_defines = row.get::<usize, Option<String>>(5).unwrap();
            Ok(DbFilesRow {
                path: path.to_path_buf(),
                cwd: Path::new(&cwd).to_path_buf(),
                binary: WrappedBinary::from_standard_binary_name(OsStr::new(&binary)).unwrap(),
                args: serde_json::from_str::<Vec<String>>(&args)
                    .unwrap()
                    .into_iter()
                    .map(OsString::from)
                    .collect(),
                local_code_file: local_code_file.map(|p| Path::new(&p).to_path_buf()),
                headers: headers.map(|h| serde_json::from_str::<Vec<PathBuf>>(&h).unwrap()),
                global_defines: global_defines
                    .map(|h| serde_json::from_str::<Vec<String>>(&h).unwrap()),
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

#[derive(Debug, Clone, Default)]
struct SourceCodeLine<'a> {
    line_number: usize,
    line: &'a str,
}

#[derive(Debug, Clone, Default)]
struct AnalysePreprocessResult<'a> {
    global_headers: Vec<PathBuf>,
    local_code: Vec<SourceCodeLine<'a>>,
}

fn find_global_include_extra_defines(global_headers: &[PathBuf], code: &str) -> Vec<String> {
    let code = code.replace("\\\n", " ");

    static INCLUDE_RE: once_cell::sync::Lazy<regex::Regex> = once_cell::sync::Lazy::new(|| {
        regex::Regex::new(r#"(?m)^#include[ \t]+["<](.*)[">]"#).unwrap()
    });
    let mut last_global_include_index = 0;
    for captures in INCLUDE_RE.captures_iter(&code) {
        let Some(header_name) = captures.get(1).map(|m| m.as_str()) else {
            continue;
        };
        if !global_headers.iter().any(|h| h.ends_with(header_name)) {
            continue;
        }
        last_global_include_index = captures.get(0).unwrap().start();
    }

    static DEFINE_RE: once_cell::sync::Lazy<regex::Regex> =
        once_cell::sync::Lazy::new(|| regex::Regex::new(r#"(?m)^#define.*$"#).unwrap());

    let mut result = vec![];
    for captures in DEFINE_RE.captures_iter(&code[..last_global_include_index]) {
        result.push(captures.get(0).unwrap().as_str().to_string());
    }
    if code.contains("#define NPY_NO_DEPRECATED_API NPY_1_7_API_VERSION") {
        result.push("#define NPY_NO_DEPRECATED_API NPY_1_7_API_VERSION".to_string());
    }
    if code.contains("#  define NANOVDB_USE_OPENVDB") {
        result.push("#define NANOVDB_USE_OPENVDB".to_string());
    }
    result
}

#[derive(Debug, Clone, Default)]
struct GccPreprocessLine<'a> {
    line_number: usize,
    header_name: &'a str,
    is_start_of_new_file: bool,
    is_return_to_file: bool,
    _next_is_system_header: bool,
    _next_is_extern_c: bool,
}

impl<'a> GccPreprocessLine<'a> {
    fn parse(line: &'a [u8]) -> Result<Self> {
        let line = std::str::from_utf8(line)?;
        let err = || anyhow::anyhow!("Failed to parse line: {:?}", line);
        static RE: once_cell::sync::Lazy<regex::Regex> = once_cell::sync::Lazy::new(|| {
            regex::Regex::new(r#"# (\d+) "(.*)"\s*(\d?)\s*(\d?)\s*(\d?)\s*(\d?)"#).unwrap()
        });
        let Some(captures) = RE.captures(line) else {
            return Err(err());
        };
        let Some(line_number) = captures.get(1).unwrap().as_str().parse::<usize>().ok() else {
            return Err(err());
        };
        let name = captures.get(2).unwrap().as_str();
        let mut numbers = vec![];
        for i in 3..=6 {
            let number_str = captures.get(i).unwrap().as_str();
            if number_str.is_empty() {
                continue;
            }
            let Some(number) = number_str.parse::<i32>().ok() else {
                return Err(err());
            };
            numbers.push(number);
        }

        Ok(GccPreprocessLine {
            line_number,
            header_name: name,
            is_start_of_new_file: numbers.contains(&1),
            is_return_to_file: numbers.contains(&2),
            _next_is_system_header: numbers.contains(&3),
            _next_is_extern_c: numbers.contains(&4),
        })
    }
}

fn has_known_include_guard(code: &[u8]) -> bool {
    let mut i = 0;
    while i < code.len() {
        let rest = &code[i..];
        if rest.starts_with(b"#pragma once") {
            return true;
        }
        if rest.starts_with(b"#ifndef") {
            return true;
        }
        if rest.starts_with(b"#include") {
            /* The include guard has to come before the first include. */
            return false;
        }
        if rest.starts_with(b"//") {
            i += rest.iter().position(|&b| b == b'\n').unwrap_or(rest.len()) + 1;
            continue;
        }
        if rest.starts_with(b"/*") {
            i += rest
                .windows(2)
                .position(|w| w == b"*/")
                .unwrap_or(rest.len())
                + 2;
            continue;
        }
        if b" \t\n".contains(&rest[0]) {
            i += 1;
            continue;
        }
        return false;
    }
    false
}

async fn is_local_header(header_path: &Path) -> bool {
    // if header_path.starts_with("/usr") {
    //     return false;
    // }
    // let Ok(code) = tokio::fs::read_to_string(header_path).await else {
    //     return false;
    // };
    // !has_known_include_guard(&code.as_bytes())
    header_path.ends_with("list_sort_impl.h")
        || header_path.ends_with("dna_rename_defs.h")
        || header_path.ends_with("dna_includes_as_strings.h")
        || header_path.ends_with("BLI_strict_flags.h")
        || header_path.ends_with("RNA_enum_items.hh")
        || header_path.ends_with("UI_icons.hh")
        || header_path.ends_with(".cc")
        || header_path.ends_with(".c")
        || header_path.ends_with("glsl_compositor_source_list.h")
        || header_path.ends_with("BLI_kdtree_impl.h")
        || header_path.ends_with("kdtree_impl.h")
        || header_path.ends_with("state_template.h")
        || header_path.ends_with("shadow_state_template.h")
        || header_path.ends_with("gpu_shader_create_info_list.hh")
        || header_path.ends_with("generic_alloc_impl.h")
        || header_path.ends_with("glsl_draw_source_list.h")
        || header_path.ends_with("compositor_shader_create_info_list.hh")
        || header_path.ends_with("glsl_gpu_source_list.h")
        || header_path.ends_with("glsl_osd_source_list.h")
        || header_path.ends_with("glsl_ocio_source_list.h")
        || header_path.ends_with("draw_debug_info.hh")
        || header_path.ends_with("draw_fullscreen_info.hh")
        || header_path.ends_with("draw_hair_refine_info.hh")
        || header_path.ends_with("draw_object_infos_info.hh")
        || header_path.ends_with("draw_view_info.hh")
        || header_path.ends_with("subdiv_info.hh")
        || header_path
            .as_os_str()
            .to_string_lossy()
            .contains("shaders/infos")
}

async fn is_local_header_with_cache(header_path: &Path, state: &Data<State>) -> bool {
    if let Some(header_type) = state.header_type_cache.lock().get(header_path) {
        return *header_type == HeaderType::Local;
    }
    let result = is_local_header(header_path).await;
    state.header_type_cache.lock().insert(
        header_path.to_owned(),
        if result {
            HeaderType::Local
        } else {
            HeaderType::Global
        },
    );
    result
}

#[tokio::test]
async fn test_is_local_header() {
    assert!(
        is_local_header(&Path::new(
            "/home/jacques/blender/blender/source/blender/blenlib/intern/list_sort_impl.h"
        ))
        .await
    );
    assert!(
        is_local_header(&Path::new(
            "/home/jacques/blender/blender/source/blender/gpu/intern/gpu_shader_create_info_list.hh"
        ))
        .await
    );
    assert!(
        !is_local_header(&Path::new(
            "/home/jacques/blender/blender/source/blender/blenlib/BLI_path_utils.hh"
        ))
        .await
    );
    assert!(!is_local_header(&Path::new("/usr/include/c++/14/cstddef")).await);
}

async fn analyse_preprocessed_file<'a>(
    code: &'a [u8],
    state: &Data<State>,
) -> Result<AnalysePreprocessResult<'a>> {
    let mut result = AnalysePreprocessResult::default();
    let mut header_stack: Vec<&str> = vec![];
    let mut local_depth = 0;
    let mut next_line = 0;

    for line in code.split(|&b| b == b'\n') {
        let is_local = header_stack.len() == local_depth;
        if line.starts_with(b"# ") {
            let preprocessor_line = GccPreprocessLine::parse(line)?;
            next_line = preprocessor_line.line_number;
            if preprocessor_line.is_start_of_new_file {
                if is_local {
                    if !is_local_header_with_cache(&Path::new(preprocessor_line.header_name), state)
                        .await
                    {
                        let header_path = PathBuf::from(preprocessor_line.header_name);
                        if !result.global_headers.contains(&header_path) {
                            result.global_headers.push(header_path);
                        }
                    } else {
                        local_depth += 1;
                    }
                }
                header_stack.push(preprocessor_line.header_name);
            } else if preprocessor_line.is_return_to_file {
                header_stack.pop();
                local_depth = local_depth.min(header_stack.len());
            }
        } else {
            if !line.is_empty() {
                if is_local {
                    result.local_code.push(SourceCodeLine {
                        line_number: next_line,
                        line: std::str::from_utf8(line)?,
                    });
                }
            }
            next_line += 1;
        }
    }
    Ok(result)
}

async fn handle_gcc_without_link_request(
    binary: WrappedBinary,
    request_gcc_args: &GCCArgs,
    cwd: &Path,
    state: &Data<State>,
) -> HttpResponse {
    let Some(request_output_path) = request_gcc_args.primary_output.as_ref() else {
        return HttpResponse::NotImplemented().body("Expected output path");
    };
    let request_output_path_clone = request_output_path.clone();

    let _log_handle = state.tasks_logger.start_task(&format!(
        "Prepare: {:?}",
        request_output_path.file_name().unwrap().to_string_lossy()
    ));

    // Do preprocessing on the provided files. This also generates a depfile that e.g. ninja will use
    // to know which headers an object file depends on.
    let mut modified_gcc_args = request_gcc_args.clone();
    modified_gcc_args.primary_output = None;
    modified_gcc_args.stop_before_link = false;
    modified_gcc_args.stop_after_preprocessing = true;

    let realized_args = modified_gcc_args.to_args();
    let realized_args_buffer = realized_args
        .iter()
        .map(|s| s.as_encoded_bytes())
        .flatten()
        .map(|b| *b)
        .collect::<Vec<_>>();
    let realized_args_hash = twox_hash::XxHash64::oneshot(0, &realized_args_buffer);
    let realized_args_hash_str = format!("{:x}", realized_args_hash);
    let preprocess_file_name = format!(
        "{}_{}.ii",
        &realized_args_hash_str,
        request_output_path_clone
            .file_name()
            .unwrap()
            .to_string_lossy()
    );
    let preprocess_file_path = state
        .data_dir
        .join("preprocessed")
        .join(&realized_args_hash_str[..2])
        .join(preprocess_file_name);
    std::fs::create_dir_all(&preprocess_file_path.parent().unwrap()).unwrap();
    let headers = Arc::new(Mutex::new(Vec::new()));
    let global_defines = Arc::new(Mutex::new(Vec::new()));

    log::info!("Preprocess: {:#?}", modified_gcc_args.to_args());
    {
        let preprocess_file_path = preprocess_file_path.clone();
        let headers = headers.clone();
        let global_defines = global_defines.clone();
        let state_clone = state.clone();
        state
            .pool
            .run(async move || {
                let result = tokio::process::Command::new(binary.to_standard_binary_name())
                    .args(&realized_args)
                    .stdin(std::process::Stdio::null())
                    .stdout(std::process::Stdio::piped())
                    .stderr(std::process::Stdio::piped())
                    .spawn()
                    .unwrap()
                    .wait_with_output()
                    .await
                    .unwrap();
                if !result.stderr.is_empty() {
                    log::error!(
                        "Preprocess failed: {}",
                        std::str::from_utf8(&result.stderr).unwrap()
                    );
                    std::process::exit(1);
                }
                let analysis = analyse_preprocessed_file(&result.stdout, &state_clone)
                    .await
                    .unwrap();
                let source_file_path = modified_gcc_args.sources.first();
                if let Some(source_file_path) = source_file_path {
                    if let Ok(source_code) = tokio::fs::read_to_string(&source_file_path.path).await
                    {
                        global_defines
                            .lock()
                            .extend(find_global_include_extra_defines(
                                &analysis.global_headers,
                                &source_code,
                            ));
                    }
                }

                headers.lock().extend(analysis.global_headers);

                let mut file = std::fs::File::create(preprocess_file_path).unwrap();
                for line in analysis.local_code {
                    if let Some(source_file_path) = source_file_path {
                        writeln!(
                            file,
                            "# {} \"{}\"",
                            line.line_number,
                            source_file_path.path.display()
                        )
                        .unwrap();
                    }
                    writeln!(file, "{}", line.line).unwrap();
                }
            })
            .await
            .unwrap();
    }

    let dummy_object = ASSETS_DIR.get_file("dummy_object.o").unwrap();
    tokio::fs::write(request_output_path, dummy_object.contents())
        .await
        .unwrap();

    store_db_file(
        &state.conn.lock(),
        &DbFilesRow {
            path: request_output_path.clone(),
            cwd: cwd.to_path_buf(),
            binary: binary,
            args: request_gcc_args.to_args(),
            local_code_file: Some(preprocess_file_path),
            headers: Some(headers.lock().clone()),
            global_defines: Some(global_defines.lock().clone()),
        },
    )
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

async fn build_combined_translation_unit(
    original_object_files: &[&Path],
    dst_object_file: &Path,
    state: &Data<State>,
) {
    let mut headers: Vec<PathBuf> = Vec::new();
    let mut preprocess_paths: Vec<PathBuf> = Vec::new();

    let mut unit_binary = WrappedBinary::Gcc;
    let mut preprocess_headers_gcc_args = GCCArgs {
        ..Default::default()
    };
    let mut compile_gcc_args = GCCArgs {
        ..Default::default()
    };

    let mut global_defines = Vec::new();

    for original_object_file in original_object_files {
        let Some(info) = load_db_file(&state.conn.lock(), &original_object_file) else {
            assert!(original_object_files.len() == 1);
            tokio::fs::copy(original_object_file, dst_object_file)
                .await
                .unwrap();
            return;
        };

        unit_binary = info.binary;
        let original_gcc_args =
            GCCArgs::parse(&original_object_file, &osstring_to_osstr_vec(&info.args)).unwrap();
        for header in &info.headers.unwrap() {
            if headers.contains(header) {
                continue;
            }
            headers.push(header.clone());
        }
        preprocess_paths.push(info.local_code_file.unwrap());

        global_defines.extend(info.global_defines.unwrap_or_default());

        preprocess_headers_gcc_args
            .user_includes
            .extend(original_gcc_args.user_includes.clone());
        compile_gcc_args
            .user_includes
            .extend(original_gcc_args.user_includes);

        preprocess_headers_gcc_args
            .system_includes
            .extend(original_gcc_args.system_includes.clone());
        compile_gcc_args
            .system_includes
            .extend(original_gcc_args.system_includes);

        preprocess_headers_gcc_args
            .f_flags
            .extend(original_gcc_args.f_flags.clone());
        compile_gcc_args.f_flags.extend(original_gcc_args.f_flags);

        preprocess_headers_gcc_args
            .g_flags
            .extend(original_gcc_args.g_flags.clone());
        compile_gcc_args.g_flags.extend(original_gcc_args.g_flags);

        preprocess_headers_gcc_args
            .opt_flags
            .extend(original_gcc_args.opt_flags.clone());
        compile_gcc_args
            .opt_flags
            .extend(original_gcc_args.opt_flags);

        preprocess_headers_gcc_args
            .defines
            .extend(original_gcc_args.defines.clone());
        compile_gcc_args.defines.extend(original_gcc_args.defines);

        preprocess_headers_gcc_args
            .machine_args
            .extend(original_gcc_args.machine_args.clone());
        compile_gcc_args
            .machine_args
            .extend(original_gcc_args.machine_args);
    }

    let (ext, lang) = if unit_binary == WrappedBinary::Gxx {
        ("ii", "c++")
    } else {
        ("i", "c")
    };

    let mut headers_code = String::new();
    for define in global_defines {
        headers_code.push_str(&define);
    }
    for header in headers {
        headers_code.push_str(&format!("#include \"{}\"\n", header.display()));
    }

    preprocess_headers_gcc_args.stop_after_preprocessing = true;
    let mut preprocess_headers_raw_args = preprocess_headers_gcc_args.to_args();
    preprocess_headers_raw_args.push("-x".into());
    preprocess_headers_raw_args.push(lang.into());
    preprocess_headers_raw_args.push("-".into());
    let mut child = tokio::process::Command::new(unit_binary.to_standard_binary_name())
        .args(&preprocess_headers_raw_args)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(headers_code.as_bytes()).await.unwrap();
    }
    let child_result = {
        log::info!("Preprocess Header: {:?}", dst_object_file.file_name());
        let _log_handle = state.tasks_logger.start_task(&format!(
            "Preprocess Header: {:?}",
            dst_object_file.file_name()
        ));
        child.wait_with_output().await.unwrap()
    };

    let mut combined_code: String = "".to_string();
    combined_code.push_str(std::str::from_utf8(&child_result.stdout).unwrap());
    combined_code.push_str("\n\n");
    for path in preprocess_paths {
        let preprocessed_code = std::fs::read_to_string(&path).unwrap();
        combined_code.push_str(&preprocessed_code);
    }

    let unit_file = tempfile::Builder::new()
        .suffix(&format!(".{}", ext))
        .tempfile()
        .unwrap();
    let unit_file_path = unit_file.path();
    tokio::fs::write(unit_file_path, combined_code)
        .await
        .unwrap();

    compile_gcc_args.primary_output = Some(dst_object_file.to_owned());
    compile_gcc_args.sources.push(SourceFile {
        path: unit_file_path.to_owned(),
        language: None,
    });
    compile_gcc_args.stop_before_link = true;

    let child = tokio::process::Command::new(unit_binary.to_standard_binary_name())
        .args(&compile_gcc_args.to_args())
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let result = {
        log::info!("Compile unit: {:#?}", compile_gcc_args.to_args());
        let _log_handle = state
            .tasks_logger
            .start_task(&format!("Compile unit: {:?}", dst_object_file.file_name()));
        child.wait_with_output().await.unwrap()
    };
    if !result.status.success() {
        log::error!(
            "Compile unit failed: {}",
            std::str::from_utf8(&result.stderr).unwrap()
        );
        std::process::exit(1);
    }
}

async fn build_wrapped_link_units(link_units: &[WrappedLinkUnit], state: &Data<State>) {
    let link_units = link_units.to_vec();
    let handles = link_units
        .into_iter()
        .map(|unit| {
            let state_clone = state.clone();
            state.pool.run(async move || {
                build_combined_translation_unit(
                    &[&unit.original_object_path],
                    &unit.wrapped_object_path,
                    &state_clone,
                )
                .await;
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
                    local_code_file: None,
                    headers: None,
                    global_defines: None,
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

    let data_dir = cli
        .data_dir
        .clone()
        .unwrap_or_else(|| PathBuf::from("./ccelerate_data"));

    std::fs::create_dir_all(&data_dir).unwrap();

    let db_migrations = Migrations::new(vec![M::up(
        "CREATE TABLE Files(
            path TEXT NOT NULL PRIMARY KEY,
            cwd TEXT NOT NULL,
            binary TEXT NOT NULL,
            args JSON NOT NULL,
            local_code_file TEXT,
            headers JSON,
            global_defines JSON
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
