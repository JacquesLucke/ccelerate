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
    CommandOutput, State, args_processing, config::Config, gcc_args, local_code::LocalCode,
    path_utils::shorten_path, task_periods::TaskPeriodInfo,
};

pub async fn wrap_compile_object_file(
    binary: WrappedBinary,
    args: &[impl AsRef<OsStr>],
    cwd: &Path,
    state: &Arc<State>,
    config: &Arc<Config>,
) -> Result<CommandOutput> {
    state
        .pool
        .run_same_thread(async move || {
            wrap_compile_object_file_impl(binary, args, cwd, state, config).await
        })
        .await
}

async fn wrap_compile_object_file_impl(
    binary: WrappedBinary,
    args: &[impl AsRef<OsStr>],
    cwd: &Path,
    state: &Arc<State>,
    config: &Arc<Config>,
) -> Result<CommandOutput> {
    let args_info = args_processing::BuildObjectFileInfo::from_args(binary, cwd, args)?;
    let local_code = extract_local_code(binary, args, cwd, state, config, &args_info).await?;
    let local_code_path = write_local_code_file(&args_info, &local_code, state).await?;
    write_dummy_object_file(&args_info.object_path).await?;

    state
        .persistent_state
        .update_object_file(&args_info.object_path, binary, cwd, args)?;
    state.persistent_state.update_object_file_local_code(
        &args_info.object_path,
        &local_code_path,
        &local_code.global_includes,
        &local_code.include_defines,
        &local_code.bad_includes.iter().collect::<Vec<_>>(),
    )?;

    Ok(CommandOutput::new_ok())
}

async fn extract_local_code(
    binary: WrappedBinary,
    args: &[impl AsRef<OsStr>],
    cwd: &Path,
    state: &Arc<State>,
    config: &Config,
    args_info: &args_processing::BuildObjectFileInfo,
) -> Result<LocalCode> {
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
    Ok(analysis)
}

async fn write_local_code_file(
    args_info: &args_processing::BuildObjectFileInfo,
    local_code: &LocalCode,
    state: &Arc<State>,
) -> Result<PathBuf> {
    let mut local_code_hash_str = format!(
        "{:x}",
        twox_hash::XxHash64::oneshot(0, &local_code.local_code)
    );
    local_code_hash_str.truncate(8);
    let debug_name = args_info
        .source_path
        .file_name()
        .unwrap_or(OsStr::new("unknown"))
        .to_string_lossy();
    let local_code_file_name = format!(
        "{}_{}.{}",
        local_code_hash_str,
        debug_name,
        args_info.source_language.to_preprocessed()?.to_valid_ext()
    );

    let preprocess_file_dir = state
        .data_dir
        .join("preprocessed")
        .join(&local_code_hash_str[..2]);
    let preprocess_file_path = preprocess_file_dir.join(local_code_file_name);
    tokio::fs::create_dir_all(preprocess_file_dir).await?;

    tokio::fs::write(&preprocess_file_path, &local_code.local_code).await?;
    Ok(preprocess_file_path)
}

async fn write_dummy_object_file(object_path: &Path) -> Result<()> {
    let dummy_object = crate::ASSETS_DIR
        .get_file("dummy_object.o")
        .expect("file should exist");
    tokio::fs::write(&object_path, dummy_object.contents()).await?;
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
