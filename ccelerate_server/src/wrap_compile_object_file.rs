#![deny(clippy::unwrap_used)]

use anyhow::Result;
use bstr::ByteSlice;
use ccelerate_shared::WrappedBinary;
use std::{
    ffi::OsStr,
    path::{Path, PathBuf},
    sync::Arc,
};

use crate::{
    CommandOutput, State, code_language::CodeLanguage, config::Config, gcc_args,
    local_code::LocalCode, path_utils::shorten_path, task_periods::TaskPeriodInfo,
};

pub async fn wrap_compile_object_file(
    binary: WrappedBinary,
    args: &[impl AsRef<OsStr>],
    cwd: &Path,
    state: &Arc<State>,
    config: &Arc<Config>,
) -> Result<CommandOutput> {
    let preprocess_result = preprocess_file_in_pool(binary, args, cwd, state, config).await?;
    let local_code_path = write_local_code_file(&preprocess_result, state).await?;
    write_dummy_object_file(&preprocess_result).await?;

    state
        .persistent_state
        .update_object_file(&preprocess_result.object, binary, cwd, args)?;
    state.persistent_state.update_object_file_local_code(
        &preprocess_result.object,
        &local_code_path,
        &preprocess_result.local_code.global_includes,
        &preprocess_result.local_code.include_defines,
        &preprocess_result
            .local_code
            .bad_includes
            .iter()
            .collect::<Vec<_>>(),
    )?;

    Ok(CommandOutput::new_ok())
}

async fn preprocess_file_in_pool(
    binary: WrappedBinary,
    args: &[impl AsRef<OsStr>],
    cwd: &Path,
    state: &Arc<State>,
    config: &Arc<Config>,
) -> Result<PreprocessFileResult> {
    let cwd = cwd.to_owned();
    let state_clone = state.clone();
    let config = config.clone();
    let args: Vec<_> = args.iter().map(|s| s.as_ref().to_owned()).collect();
    state
        .pool
        .run(async move || preprocess_file(binary, &args, &cwd, &state_clone, &config).await)
        .await?
}

struct PreprocessFileResult {
    /// The path to the source translation unit.
    source: PathBuf,
    /// The path to where the object file would have been written usually.
    object: PathBuf,
    /// The language of the preprocessed local code.
    local_code_language: CodeLanguage,
    /// The local code of this translation unit.
    local_code: LocalCode,
}

async fn preprocess_file(
    binary: WrappedBinary,
    args: &[impl AsRef<OsStr>],
    cwd: &Path,
    state: &Arc<State>,
    config: &Config,
) -> Result<PreprocessFileResult> {
    let args_info = gcc_args::BuildObjectFileInfo::from_args(cwd, args)?;
    let preprocessed_language = args_info.source_language.to_preprocessed()?;

    let task_period = state.task_periods.start(PreprocessTranslationUnitTaskInfo {
        dst_object_file: args_info.object_path.clone(),
    });

    let preprocessing_args =
        gcc_args::update_build_object_args_to_output_preprocessed_with_defines(args)?;

    let child = tokio::process::Command::new(binary.to_standard_binary_name())
        .args(preprocessing_args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .current_dir(cwd)
        .spawn()?;
    let child_result = child.wait_with_output().await?;
    if !child_result.status.success() {
        return Err(CommandOutput::from_process_output(child_result).into());
    }
    let preprocessed_code = child_result.stdout;
    task_period.finished_successfully();
    let task_period = state
        .task_periods
        .start(HandlePreprocessedTranslationUnitTaskInfo {
            dst_object_file: args_info.object_path.clone(),
        });
    let analysis = LocalCode::from_preprocessed_code(
        preprocessed_code.as_bstr(),
        &args_info.source_path,
        config,
    )
    .await?;

    task_period.finished_successfully();
    Ok(PreprocessFileResult {
        source: args_info.source_path,
        local_code_language: preprocessed_language,
        object: args_info.object_path.clone(),
        local_code: analysis,
    })
}

async fn write_local_code_file(
    preprocess_result: &PreprocessFileResult,
    state: &Arc<State>,
) -> Result<PathBuf> {
    let mut local_code_hash_str = format!(
        "{:x}",
        twox_hash::XxHash64::oneshot(0, &preprocess_result.local_code.local_code)
    );
    local_code_hash_str.truncate(8);
    let debug_name = preprocess_result
        .source
        .file_name()
        .unwrap_or(OsStr::new("unknown"))
        .to_string_lossy();
    let local_code_file_name = format!(
        "{}_{}.{}",
        local_code_hash_str,
        debug_name,
        preprocess_result.local_code_language.to_valid_ext()
    );

    let preprocess_file_dir = state
        .data_dir
        .join("preprocessed")
        .join(&local_code_hash_str[..2]);
    let preprocess_file_path = preprocess_file_dir.join(local_code_file_name);
    tokio::fs::create_dir_all(preprocess_file_dir).await?;

    tokio::fs::write(
        &preprocess_file_path,
        &preprocess_result.local_code.local_code,
    )
    .await?;
    Ok(preprocess_file_path)
}

async fn write_dummy_object_file(preprocess_result: &PreprocessFileResult) -> Result<()> {
    let dummy_object = crate::ASSETS_DIR
        .get_file("dummy_object.o")
        .expect("file should exist");
    tokio::fs::write(&preprocess_result.object, dummy_object.contents()).await?;
    Ok(())
}

struct PreprocessTranslationUnitTaskInfo {
    dst_object_file: PathBuf,
}

impl TaskPeriodInfo for PreprocessTranslationUnitTaskInfo {
    fn category(&self) -> String {
        "Preprocess".to_string()
    }

    fn terminal_one_liner(&self) -> String {
        shorten_path(&self.dst_object_file)
    }

    fn log_detailed(&self) {
        log::info!("Preprocess: {}", self.dst_object_file.to_string_lossy());
    }
}

struct HandlePreprocessedTranslationUnitTaskInfo {
    dst_object_file: PathBuf,
}

impl TaskPeriodInfo for HandlePreprocessedTranslationUnitTaskInfo {
    fn category(&self) -> String {
        "Local Code".to_string()
    }

    fn terminal_one_liner(&self) -> String {
        shorten_path(&self.dst_object_file)
    }

    fn log_detailed(&self) {
        log::info!(
            "Handle preprocessed: {}",
            self.dst_object_file.to_string_lossy()
        );
    }
}
