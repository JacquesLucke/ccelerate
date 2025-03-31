#![deny(clippy::unwrap_used)]

use actix_web::{HttpResponse, web::Data};
use anyhow::Result;
use bstr::{BStr, BString, ByteSlice};
use ccelerate_shared::WrappedBinary;
use parking_lot::Mutex;
use std::{
    collections::HashSet,
    ffi::OsStr,
    io::Write,
    path::{Path, PathBuf},
    process::ExitStatus,
    sync::Arc,
};

use crate::{
    State,
    code_language::CodeLanguage,
    config::Config,
    database::{FileRecord, store_file_record},
    gcc_args, log_file,
    source_file::SourceFile,
    task_log::{TaskInfo, log_task},
};

#[derive(Debug, Default)]
struct LocalCodeResult {
    // Preprocessed code of the source file without any of the headers.
    local_code: BString,
    // Global headers that are included in this file. Generally, all these headers
    // should have include guards and their include order should not matter.
    // This also includes standard library headers.
    global_includes: Vec<PathBuf>,
    // Sometimes, implementation files define values that affect headers that are typically global.
    // E.g. `#define DNA_DEPRECATED_ALLOW` in Blender.
    include_defines: Vec<BString>,
    // Some headers are bad because they define symbols aliases that easily conflict with other code.
    bad_includes: HashSet<PathBuf>,
}

async fn parse_preprocessed_source(
    code: &BStr,
    source_file_path: &Path,
    config: &Config,
) -> Result<LocalCodeResult> {
    let mut result = LocalCodeResult::default();

    writeln!(result.local_code, "#pragma GCC diagnostic push")?;

    let mut header_stack: Vec<&Path> = Vec::new();
    let mut local_depth = 0;

    let mut revertable_previous_line_start = None;
    let write_line_markers = true;

    for line in code.split(|&b| b == b'\n') {
        let is_local = header_stack.len() == local_depth;
        let line = line.as_bstr();
        if line.starts_with(b"#define ") {
            if is_local {
                if let Ok(macro_def) = MacroDefinition::parse(line) {
                    if config.is_include_define(macro_def.name) {
                        result.include_defines.push(line.to_owned());
                    }
                }
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
                    if config.is_local_header(header_path) {
                        local_depth += 1;
                    } else {
                        result.global_includes.push(header_path.to_owned());
                    }
                }
                if !result.bad_includes.contains(header_path)
                    && config.has_bad_global_symbol(header_path)
                {
                    result.bad_includes.insert(header_path.to_owned());
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
    writeln!(result.local_code, "#pragma GCC diagnostic pop")?;
    Ok(result)
}

#[derive(Debug, Clone)]
struct MacroDefinition<'a> {
    name: &'a BStr,
    _value: &'a BStr,
}

impl<'a> MacroDefinition<'a> {
    fn parse(line: &'a BStr) -> Result<Self> {
        static RE: once_cell::sync::Lazy<regex::bytes::Regex> = once_cell::sync::Lazy::new(|| {
            regex::bytes::Regex::new(r#"(?m)^#define\s+(\w+)(.*)$"#).expect("should be valid")
        });
        let Some(captures) = RE.captures(line) else {
            return Err(anyhow::anyhow!("Failed to parse line: {:?}", line));
        };
        let name = captures
            .get(1)
            .expect("group should exist")
            .as_bytes()
            .as_bstr();
        let value = captures
            .get(2)
            .expect("group should exist")
            .as_bytes()
            .as_bstr();
        Ok(MacroDefinition {
            name,
            _value: value,
        })
    }
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

struct PreprocessFileResult {
    source_file: SourceFile,
    preprocessed_language: CodeLanguage,
    original_obj_output: PathBuf,
    analysis: LocalCodeResult,
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
    FailedParse,
}

async fn preprocess_file<S: AsRef<OsStr>>(
    binary: WrappedBinary,
    build_object_file_args: &[S],
    cwd: &Path,
    state: &Data<State>,
    config: &Config,
) -> Result<PreprocessFileResult, PreprocessFileError> {
    let Ok(args_info) = gcc_args::BuildObjectFileInfo::from_args(cwd, build_object_file_args)
    else {
        return Err(PreprocessFileError::FailedParse);
    };
    let Ok(preprocessed_language) = args_info.source_language.to_preprocessed() else {
        return Err(PreprocessFileError::CannotPreprocessLanguage);
    };

    let task_period = log_task(
        &PreprocessTranslationUnitTaskInfo {
            dst_object_file: args_info.object_path.clone(),
        },
        state,
    );

    let Ok(preprocessing_args) =
        gcc_args::update_build_object_args_to_output_preprocessed_with_defines(
            build_object_file_args,
        )
    else {
        return Err(PreprocessFileError::FailedParse);
    };

    let Ok(child) = tokio::process::Command::new(binary.to_standard_binary_name())
        .args(preprocessing_args)
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
    if state.cli.log_files {
        let _ = log_file(
            state,
            &format!("Preprocessed {}", args_info.object_path.display()),
            &preprocessed_code,
            preprocessed_language.to_valid_ext(),
        )
        .await;
    }
    task_period.finished_successfully();
    let task_period = log_task(
        &HandlePreprocessedTranslationUnitTaskInfo {
            dst_object_file: args_info.object_path.clone(),
        },
        state,
    );
    let Ok(analysis) =
        parse_preprocessed_source(preprocessed_code.as_bstr(), &args_info.source_path, config)
            .await
    else {
        return Err(PreprocessFileError::AnalysisFailed);
    };

    task_period.finished_successfully();
    Ok(PreprocessFileResult {
        source_file: SourceFile {
            path: args_info.source_path,
            language_override: None,
        },
        preprocessed_language,
        original_obj_output: args_info.object_path.clone(),
        analysis,
    })
}

pub async fn handle_gcc_without_link_request<S: AsRef<OsStr>>(
    binary: WrappedBinary,
    build_object_file_args: &[S],
    cwd: &Path,
    state: &Data<State>,
    config: &Arc<Config>,
) -> HttpResponse {
    let preprocess_result = Arc::new(Mutex::new(None));
    {
        let cwd = cwd.to_owned();
        let preprocess_result = preprocess_result.clone();
        let state_clone = state.clone();
        let config = config.clone();
        let build_object_file_args: Vec<_> = build_object_file_args
            .iter()
            .map(|s| s.as_ref().to_owned())
            .collect();
        let Ok(_) = state
            .pool
            .run(async move || {
                let result =
                    preprocess_file(binary, &build_object_file_args, &cwd, &state_clone, &config)
                        .await;
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
        twox_hash::XxHash64::oneshot(0, &preprocess_result.analysis.local_code)
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
    match tokio::fs::create_dir_all(preprocess_file_dir).await {
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

    let Ok(_) = tokio::fs::write(
        &preprocess_file_path,
        &preprocess_result.analysis.local_code,
    )
    .await
    else {
        return HttpResponse::InternalServerError().body("Failed to write local code");
    };

    let Ok(_) = store_file_record(
        &state.conn.lock(),
        &preprocess_result.original_obj_output,
        &FileRecord {
            cwd: cwd.to_path_buf(),
            binary,
            args: build_object_file_args
                .iter()
                .map(|s| s.as_ref().to_owned())
                .collect(),
            local_code_file: Some(preprocess_file_path),
            global_includes: Some(preprocess_result.analysis.global_includes),
            include_defines: Some(preprocess_result.analysis.include_defines),
            bad_includes: Some(
                preprocess_result
                    .analysis
                    .bad_includes
                    .into_iter()
                    .collect(),
            ),
        },
    ) else {
        return HttpResponse::InternalServerError().body("Failed to store db file");
    };

    HttpResponse::Ok().json(&ccelerate_shared::RunResponseDataWire {
        ..Default::default()
    })
}

struct PreprocessTranslationUnitTaskInfo {
    dst_object_file: PathBuf,
}

impl TaskInfo for PreprocessTranslationUnitTaskInfo {
    fn category(&self) -> String {
        "Preprocess".to_string()
    }

    fn short_name(&self) -> String {
        format!(
            "Preprocess: {}",
            self.dst_object_file
                .file_name()
                .unwrap_or(OsStr::new("unknown"))
                .to_string_lossy()
        )
    }

    fn log(&self) {
        log::info!("Preprocess: {}", self.dst_object_file.to_string_lossy());
    }
}

struct HandlePreprocessedTranslationUnitTaskInfo {
    dst_object_file: PathBuf,
}

impl TaskInfo for HandlePreprocessedTranslationUnitTaskInfo {
    fn category(&self) -> String {
        "Local Code".to_string()
    }

    fn short_name(&self) -> String {
        format!(
            "Handle preprocessed: {}",
            self.dst_object_file
                .file_name()
                .unwrap_or(OsStr::new("unknown"))
                .to_string_lossy()
        )
    }

    fn log(&self) {
        log::info!(
            "Handle preprocessed: {}",
            self.dst_object_file.to_string_lossy()
        );
    }
}
