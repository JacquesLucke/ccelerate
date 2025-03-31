#![deny(clippy::unwrap_used)]

use actix_web::{HttpResponse, web::Data};
use anyhow::Result;
use bstr::ByteSlice;
use ccelerate_shared::WrappedBinary;
use parking_lot::Mutex;
use std::{
    ffi::OsStr,
    path::{Path, PathBuf},
    process::ExitStatus,
    sync::Arc,
};

use crate::{
    State,
    code_language::CodeLanguage,
    config::Config,
    database::{FileRecord, store_file_record},
    gcc_args,
    local_code::LocalCode,
    log_file,
    source_file::SourceFile,
    task_log::{TaskInfo, log_task},
};

struct PreprocessFileResult {
    source_file: SourceFile,
    preprocessed_language: CodeLanguage,
    original_obj_output: PathBuf,
    analysis: LocalCode,
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
    let Ok(analysis) = LocalCode::from_preprocessed_code(
        preprocessed_code.as_bstr(),
        &args_info.source_path,
        config,
    )
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
