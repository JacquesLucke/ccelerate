#![deny(clippy::unwrap_used)]

use actix_web::{HttpResponse, http::header, web::Data};
use anyhow::Result;
use bstr::{BStr, BString, ByteSlice};
use ccelerate_shared::WrappedBinary;
use parking_lot::Mutex;
use ratatui::symbols::line;
use std::{
    collections::HashSet,
    ffi::OsStr,
    io::Write,
    path::{Path, PathBuf},
    process::ExitStatus,
    sync::Arc,
};

use crate::{
    DbFilesRow, DbFilesRowData, HeaderType, State,
    parse_gcc::{GCCArgs, Language, SourceFile},
};

#[derive(Debug, Default)]
struct ParsePreprocessResult {
    // Preprocessed code of the source file without any of the headers.
    local_code: BString,
    // Global headers that are included in this file. Generally, all these headers
    // should have include guards and their include order should not matter.
    // This also includes standard library headers.
    global_includes: Vec<PathBuf>,
    // Sometimes, implementation files define values that affect headers that are typically global.
    // E.g. `#define DNA_DEPRECATED_ALLOW` in Blender.
    include_defines: Vec<BString>,
    config_includes: Vec<PathBuf>,
}

async fn parse_preprocessed_source(
    code: &BStr,
    source_file_path: &Path,
    state: &Data<State>,
) -> Result<ParsePreprocessResult> {
    let mut result = ParsePreprocessResult::default();

    let mut header_stack: Vec<&Path> = Vec::new();
    let mut local_depth = 0;

    let mut revertable_previous_line_start = None;
    let write_line_markers = true;

    for line in code.split(|&b| b == b'\n') {
        let is_local = header_stack.len() == local_depth;
        let line = line.as_bstr();
        if let Some(_definition) = line.strip_prefix(b"#define ") {
            if is_local && state.config.lock().is_include_define(line) {
                result.include_defines.push(line.to_owned());
            }
        } else if let Some(_undef) = line.strip_prefix(b"#undef ") {
            continue;
        } else if line.starts_with(b"# ") {
            let Ok(line_marker) = GccLinemarker::parse(line) else {
                continue;
            };
            let header_path = Path::new(line_marker.header_name);
            if line_marker.is_start_of_new_file {
                if is_local {
                    if state.config.lock().is_config_header(header_path) {
                        result.config_includes.push(header_path.to_owned());
                    } else if state.config.lock().is_local_header(header_path) {
                        local_depth += 1;
                    } else {
                        result.global_includes.push(header_path.to_owned());
                    }
                }
                header_stack.push(header_path);
            } else if line_marker.is_return_to_file {
                header_stack.pop();
                local_depth = local_depth.min(header_stack.len());
            }
            if write_line_markers && header_stack.len() == local_depth {
                if let Some(len) = revertable_previous_line_start {
                    // Remove the previously written line marker because it does not have a purpose
                    // oi the next line contains a line marker as well.
                    result.local_code.truncate(len);
                }
                let file_path = header_stack.last().unwrap_or(&source_file_path);
                revertable_previous_line_start = Some(result.local_code.len());
                writeln!(
                    result.local_code,
                    "# {} \"{}\"",
                    line_marker.line_number,
                    file_path.display()
                )?;
            }
        } else if is_local {
            writeln!(result.local_code, "{}", line)?;
            if !line.trim_ascii().is_empty() {
                revertable_previous_line_start = None;
            }
        }
    }
    Ok(result)
}

#[derive(Debug, Clone, Default)]
struct AnalysePreprocessResult {
    global_headers: Vec<PathBuf>,
}

fn find_global_include_extra_defines(global_headers: &[PathBuf], code: &BStr) -> Vec<String> {
    let code = code.replace(b"\\\n", b" ");
    let code = code.as_bstr();

    static INCLUDE_RE: once_cell::sync::Lazy<regex::bytes::Regex> =
        once_cell::sync::Lazy::new(|| {
            regex::bytes::Regex::new(r#"(?m)^#include[ \t]+["<](.*)[">]"#).expect("should be valid")
        });
    let mut last_global_include_index = 0;
    for captures in INCLUDE_RE.captures_iter(code) {
        let Some(header_name) = captures.get(1).map(|m| m.as_bytes()) else {
            continue;
        };
        let Ok(header_name) = header_name.to_str() else {
            continue;
        };
        if !global_headers.iter().any(|h| h.ends_with(header_name)) {
            continue;
        }
        last_global_include_index = captures.get(0).expect("group should exist").start();
    }

    static DEFINE_RE: once_cell::sync::Lazy<regex::bytes::Regex> =
        once_cell::sync::Lazy::new(|| {
            regex::bytes::Regex::new(r#"(?m)^#define.*$"#).expect("should be valid")
        });

    let mut result = vec![];
    for captures in DEFINE_RE.captures_iter(&code[..last_global_include_index]) {
        let Some(define) = captures.get(0).map(|m| m.as_bytes()) else {
            continue;
        };
        let Ok(define) = define.to_str() else {
            continue;
        };
        result.push(define.to_owned());
    }
    if code.contains_str(b"#define NPY_NO_DEPRECATED_API NPY_1_7_API_VERSION") {
        result.push("#define NPY_NO_DEPRECATED_API NPY_1_7_API_VERSION".to_string());
    }
    if code.contains_str(b"#  define NANOVDB_USE_OPENVDB") {
        result.push("#define NANOVDB_USE_OPENVDB".to_string());
    }
    result
}

#[derive(Debug, Clone, Default)]
struct GccLinemarker<'a> {
    line_number: usize,
    header_name: &'a str,
    is_start_of_new_file: bool,
    is_return_to_file: bool,
    _next_is_system_header: bool,
    _next_is_extern_c: bool,
}

impl<'a> GccLinemarker<'a> {
    fn parse(line: &'a BStr) -> Result<Self> {
        let line = std::str::from_utf8(line)?;
        let err = || anyhow::anyhow!("Failed to parse line: {:?}", line);
        static RE: once_cell::sync::Lazy<regex::Regex> = once_cell::sync::Lazy::new(|| {
            regex::Regex::new(r#"# (\d+) "(.*)"\s*(\d?)\s*(\d?)\s*(\d?)\s*(\d?)"#)
                .expect("should be valid")
        });
        let Some(captures) = RE.captures(line) else {
            return Err(err());
        };
        let Some(line_number) = captures
            .get(1)
            .expect("group should exist")
            .as_str()
            .parse::<usize>()
            .ok()
        else {
            return Err(err());
        };
        let name = captures.get(2).expect("group should exist").as_str();
        let mut numbers = vec![];
        for i in 3..=6 {
            let number_str = captures.get(i).expect("group should exist").as_str();
            if number_str.is_empty() {
                continue;
            }
            let Some(number) = number_str.parse::<i32>().ok() else {
                return Err(err());
            };
            numbers.push(number);
        }

        Ok(GccLinemarker {
            line_number,
            header_name: name,
            is_start_of_new_file: numbers.contains(&1),
            is_return_to_file: numbers.contains(&2),
            _next_is_system_header: numbers.contains(&3),
            _next_is_extern_c: numbers.contains(&4),
        })
    }
}

async fn is_local_header_with_cache(header_path: &Path, state: &Data<State>) -> bool {
    if let Some(header_type) = state.header_type_cache.lock().get(header_path) {
        return *header_type == HeaderType::Local;
    }
    let result = state.config.lock().is_local_header(header_path);
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

async fn analyse_preprocessed_file(
    code: &[u8],
    state: &Data<State>,
) -> Result<AnalysePreprocessResult> {
    let mut result = AnalysePreprocessResult::default();
    let mut header_stack: Vec<&str> = vec![];
    let mut local_depth = 0;
    let mut next_line = 0;

    for line in code.split(|&b| b == b'\n') {
        let is_local = header_stack.len() == local_depth;
        if line.starts_with(b"# ") {
            let preprocessor_line = GccLinemarker::parse(line.as_bstr())?;
            next_line = preprocessor_line.line_number;
            if preprocessor_line.is_start_of_new_file {
                if is_local {
                    if !is_local_header_with_cache(Path::new(preprocessor_line.header_name), state)
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
            if !line.is_empty() && is_local {}
            next_line += 1;
        }
    }
    Ok(result)
}

struct PreprocessFileResult {
    source_file: SourceFile,
    preprocessed_language: Language,
    local_code: BString,
    headers: Vec<PathBuf>,
    header_defines: Vec<String>,
    original_obj_output: PathBuf,
}

#[allow(dead_code)]
#[derive(Debug)]
enum PreprocessFileError {
    MissingPrimaryOutput,
    FailedToSpawn,
    FailedToWaitForChild,
    MultipleSourceFiles,
    NoSourceFile,
    FailedToReadSourceFile {
        path: PathBuf,
        err: tokio::io::Error,
    },
    AnalysisFailed,
    FailedToDetermineLanguage,
    CannotPreprocessLanguage,
    PreprocessorFailed {
        stdout: Vec<u8>,
        stderr: Vec<u8>,
        status: ExitStatus,
    },
}

fn update_gcc_args_for_preprocessing(build_object_file_args: GCCArgs) -> GCCArgs {
    // Do not disable generating depfiles which are used by Ninja.
    let mut result = build_object_file_args;
    result.primary_output = None;
    result.stop_before_link = false;
    result.stop_after_preprocessing = true;
    result.preprocess_keep_defines = true;
    result
}

async fn preprocess_file(
    binary: WrappedBinary,
    build_object_file_args: &GCCArgs,
    cwd: &Path,
    state: &Data<State>,
) -> Result<PreprocessFileResult, PreprocessFileError> {
    if build_object_file_args.sources.len() >= 2 {
        return Err(PreprocessFileError::MultipleSourceFiles);
    }
    let Some(source_file) = build_object_file_args.sources.first() else {
        return Err(PreprocessFileError::NoSourceFile);
    };
    let source_file_code = match tokio::fs::read(&source_file.path).await {
        Ok(code) => code,
        Err(err) => {
            return Err(PreprocessFileError::FailedToReadSourceFile {
                path: source_file.path.clone(),
                err,
            });
        }
    };
    let Ok(source_file_language) = source_file.language() else {
        return Err(PreprocessFileError::FailedToDetermineLanguage);
    };
    let Ok(preprocessed_language) = source_file_language.to_preprocessed() else {
        return Err(PreprocessFileError::CannotPreprocessLanguage);
    };
    let Some(obj_path) = build_object_file_args.primary_output.as_ref() else {
        return Err(PreprocessFileError::MissingPrimaryOutput);
    };

    let _log_handle = state
        .tasks_logger
        .start_task(&format!("Preprocess: {}", source_file.path.display()));

    let preprocessing_args = update_gcc_args_for_preprocessing(build_object_file_args.clone());

    let Ok(child) = tokio::process::Command::new(binary.to_standard_binary_name())
        .args(preprocessing_args.to_args())
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .current_dir(cwd)
        .spawn()
    else {
        return Err(PreprocessFileError::FailedToSpawn);
    };
    let Ok(child_result) = child.wait_with_output().await else {
        return Err(PreprocessFileError::FailedToWaitForChild);
    };
    if !child_result.status.success() {
        return Err(PreprocessFileError::PreprocessorFailed {
            stdout: child_result.stdout,
            stderr: child_result.stderr,
            status: child_result.status,
        });
    }
    let preprocessed_code = child_result.stdout;
    let Ok(data) =
        parse_preprocessed_source(preprocessed_code.as_bstr(), &source_file.path, state).await
    else {
        return Err(PreprocessFileError::AnalysisFailed);
    };
    let Ok(analysis) = analyse_preprocessed_file(&preprocessed_code, state).await else {
        return Err(PreprocessFileError::AnalysisFailed);
    };
    let defines =
        find_global_include_extra_defines(&analysis.global_headers, source_file_code.as_bstr());

    Ok(PreprocessFileResult {
        source_file: source_file.clone(),
        preprocessed_language,
        local_code: data.local_code,
        headers: analysis.global_headers,
        header_defines: defines,
        original_obj_output: obj_path.clone(),
    })
}

pub async fn handle_gcc_without_link_request(
    binary: WrappedBinary,
    build_object_file_args: &GCCArgs,
    cwd: &Path,
    state: &Data<State>,
) -> HttpResponse {
    let preprocess_result = Arc::new(Mutex::new(None));
    {
        let cwd = cwd.to_owned();
        let build_object_file_args = build_object_file_args.clone();
        let preprocess_result = preprocess_result.clone();
        let state_clone = state.clone();
        let Ok(_) = state
            .pool
            .run(async move || {
                let result =
                    preprocess_file(binary, &build_object_file_args, &cwd, &state_clone).await;
                preprocess_result.lock().replace(result);
            })
            .await
        else {
            return HttpResponse::InternalServerError().body("Failed to await preprocessing");
        };
    }
    let preprocess_result = match preprocess_result.lock().take() {
        Some(Ok(result)) => result,
        Some(Err(PreprocessFileError::PreprocessorFailed {
            stdout,
            stderr,
            status,
        })) => {
            return HttpResponse::Ok().json(
                ccelerate_shared::RunResponseData {
                    stdout,
                    stderr,
                    status: status.code().unwrap_or(1),
                }
                .to_wire(),
            );
        }
        Some(Err(e)) => {
            return HttpResponse::BadRequest().body(format!("Failed to preprocess file: {:?}", e));
        }
        None => {
            return HttpResponse::InternalServerError().body("Failed to preprocess file");
        }
    };

    let local_code_hash_str = format!(
        "{:x}",
        twox_hash::XxHash64::oneshot(0, &preprocess_result.local_code)
    );
    let debug_name = preprocess_result
        .source_file
        .path
        .file_name()
        .unwrap_or(OsStr::new("unknown"))
        .to_string_lossy();
    let local_code_file_name = format!(
        "{}_{}.{}",
        local_code_hash_str,
        debug_name,
        preprocess_result.preprocessed_language.to_valid_ext()
    );

    let preprocess_file_dir = state
        .data_dir
        .join("preprocessed")
        .join(&local_code_hash_str[..2]);
    let preprocess_file_path = preprocess_file_dir.join(local_code_file_name);
    match std::fs::create_dir_all(preprocess_file_dir) {
        Ok(()) => {}
        Err(e) => {
            return HttpResponse::InternalServerError()
                .body(format!("Failed to create preprocess file directory: {e}"));
        }
    }

    let dummy_object = crate::ASSETS_DIR
        .get_file("dummy_object.o")
        .expect("file should exist");
    match tokio::fs::write(
        &preprocess_result.original_obj_output,
        dummy_object.contents(),
    )
    .await
    {
        Ok(_) => {}
        Err(e) => {
            return HttpResponse::InternalServerError().body(format!(
                "Failed to write dummy object to {}: {}",
                preprocess_result.original_obj_output.display(),
                e
            ));
        }
    }

    let Ok(_) = tokio::fs::write(&preprocess_file_path, &preprocess_result.local_code).await else {
        return HttpResponse::InternalServerError().body("Failed to write local code");
    };

    let Ok(_) = crate::store_db_file(
        &state.conn.lock(),
        &DbFilesRow {
            path: preprocess_result.original_obj_output,
            data: DbFilesRowData {
                cwd: cwd.to_path_buf(),
                binary,
                args: build_object_file_args.to_args(),
                local_code_file: Some(preprocess_file_path),
                headers: Some(preprocess_result.headers),
                global_defines: Some(preprocess_result.header_defines),
            },
        },
    ) else {
        return HttpResponse::InternalServerError().body("Failed to store db file");
    };

    HttpResponse::Ok().json(&ccelerate_shared::RunResponseDataWire {
        ..Default::default()
    })
}
