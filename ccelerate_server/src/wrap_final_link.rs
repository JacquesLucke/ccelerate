#![deny(clippy::unwrap_used)]

use std::{
    ffi::OsStr,
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::Result;
use ccelerate_shared::WrappedBinary;
use futures::stream::FuturesUnordered;
use nunny::NonEmpty;

use crate::{
    CommandOutput, ar_args, args_processing,
    code_language::CodeLanguage,
    config::Config,
    gcc_args,
    group_compatible_objects::group_compatible_objects,
    link_sources::find_link_sources,
    path_utils::{self, shorten_path},
    preprocess_headers::get_preprocessed_headers,
    state::State,
    state_persistent::ObjectData,
    task_periods::TaskPeriodInfo,
};

pub async fn wrap_final_link(
    binary: WrappedBinary,
    original_args: &[impl AsRef<OsStr>],
    cwd: &Path,
    state: &Arc<State>,
    config: &Arc<Config>,
) -> Result<CommandOutput> {
    let args_info = args_processing::LinkFileInfo::from_args(binary, cwd, original_args)?;
    let link_sources = find_link_sources(&args_info, state)?;
    let object_paths =
        compile_objects_smart(&link_sources.known_object_files, state, config).await?;
    let archive_path = create_thin_archive_for_objects(&object_paths, state).await?;

    let mut all_link_sources = vec![archive_path];
    all_link_sources.extend(link_sources.unknown_sources.into_iter());

    final_link(
        binary,
        original_args,
        &args_info,
        cwd,
        state,
        &all_link_sources,
    )
    .await
}

async fn compile_objects_smart(
    objects: &[Arc<ObjectData>],
    state: &Arc<State>,
    config: &Arc<Config>,
) -> Result<Vec<PathBuf>> {
    let compatible_objects_groups = group_compatible_objects(objects, state)?;
    let handles = FuturesUnordered::new();
    for compatible_objects in compatible_objects_groups {
        let state = state.clone();
        let config = config.clone();
        let handle = tokio::task::spawn(async move {
            compile_compatible_objects_in_chunks(&compatible_objects.objects, &state, &config).await
        });
        handles.push(handle);
    }
    let mut objects = Vec::new();
    for handle in handles {
        objects.extend(handle.await??);
    }
    Ok(objects)
}

#[async_recursion::async_recursion]
async fn compile_compatible_objects_in_chunks(
    compatible_objects: &NonEmpty<[Arc<ObjectData>]>,
    state: &Arc<State>,
    config: &Arc<Config>,
) -> Result<Vec<PathBuf>> {
    if compatible_objects.is_empty() {
        return Ok(vec![]);
    }
    if compatible_objects.len() <= 10 {
        let key = compatible_objects
            .iter()
            .map(|o| o.path.as_path())
            .collect::<Vec<_>>();
        let latest_build = compatible_objects
            .iter()
            .map(|o| o.last_build)
            .max()
            .expect("never empty");
        let result = state
            .objects_cache
            .get(&key, latest_build, async || {
                compile_compatible_objects_in_pool(state, compatible_objects, config).await
            })
            .await;
        match result.as_ref() {
            Ok(object_path) => {
                let object_path = object_path.clone();
                return Ok(vec![object_path]);
            }
            Err(e) => {
                if compatible_objects.len() == 1 {
                    return Err(anyhow::anyhow!("{}", e));
                }
            }
        }
    }
    let (left, right) = compatible_objects.split_at(compatible_objects.len() / 2);
    let left = NonEmpty::<[_]>::new(left).expect("empty");
    let right = NonEmpty::<[_]>::new(right).expect("empty");
    let (left, right) = tokio::try_join!(
        compile_compatible_objects_in_chunks(left, state, config),
        compile_compatible_objects_in_chunks(right, state, config)
    )?;
    Ok(left.into_iter().chain(right).collect())
}

async fn compile_compatible_objects_in_pool(
    state: &Arc<State>,
    objects: &NonEmpty<[Arc<ObjectData>]>,
    config: &Arc<Config>,
) -> Result<PathBuf> {
    let state_clone = state.clone();
    let objects = nunny::Vec::new(objects.to_vec()).expect("empty");
    let config = config.clone();
    state
        .pool
        .run_spawned(async move || {
            compile_compatible_objects(&state_clone, &objects, &config).await
        })
        .await?
}

async fn compile_compatible_objects(
    state: &Arc<State>,
    objects: &NonEmpty<[Arc<ObjectData>]>,
    config: &Config,
) -> Result<PathBuf> {
    let any_object = objects.first();
    let preprocessed_language = CodeLanguage::from_path(&any_object.local_code.local_code_file)?;

    let object_name = format!("{}.o", uuid::Uuid::new_v4());
    let object_path = state
        .data_dir
        .join("objects")
        .join(&object_name[..2])
        .join(object_name);
    path_utils::ensure_directory_for_file(&object_path).await?;

    let preprocessed_source_file =
        tempfile::NamedTempFile::with_suffix(format!(".{}", preprocessed_language.valid_ext()))?;
    get_preprocessed_headers(objects, state, config, preprocessed_source_file.path()).await?;
    let mut input_file = tokio::fs::OpenOptions::new()
        .append(true)
        .open(preprocessed_source_file.path())
        .await?;

    let task_period = state.task_periods.start(CompileChunkTaskInfo {
        sources: objects
            .iter()
            .map(|r| r.local_code.local_code_file.clone())
            .collect(),
    });

    for object in objects {
        tokio::io::copy(
            &mut tokio::fs::File::open(&object.local_code.local_code_file).await?,
            &mut input_file,
        )
        .await?;
    }

    let build_args = gcc_args::update_to_build_object_from_stdin(
        &any_object.create.args,
        preprocessed_source_file.path(),
        &object_path,
    )?;

    let child_output =
        tokio::process::Command::new(any_object.create.binary.to_standard_binary_name())
            .args(build_args)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()?
            .wait_with_output()
            .await?;
    if !child_output.status.success() {
        return Err(CommandOutput::from_process_output(child_output).into());
    }
    task_period.finished_successfully();
    Ok(object_path)
}

pub async fn create_thin_archive_for_objects(
    objects: &[PathBuf],
    state: &Arc<State>,
) -> Result<PathBuf> {
    let task_period = state.task_periods.start(CreateThinArchiveTaskInfo {});

    let archive_name = format!("{}.a", uuid::Uuid::new_v4());
    let archive_path = state
        .data_dir
        .join("archives")
        .join(&archive_name[..2])
        .join(archive_name);
    path_utils::ensure_directory_for_file(&archive_path).await?;

    let child = tokio::process::Command::new(WrappedBinary::Ar.to_standard_binary_name())
        .args(ar_args::make_args_to_build_thin_static_archive(
            &archive_path,
            objects,
        ))
        .spawn()?;
    let child_output = child.wait_with_output().await?;
    if !child_output.status.success() {
        return Err(anyhow::anyhow!(
            "Archive creation failed: {}",
            String::from_utf8_lossy(&child_output.stderr)
        ));
    }

    task_period.finished_successfully();
    Ok(archive_path)
}

pub async fn final_link(
    binary: WrappedBinary,
    original_args: &[impl AsRef<OsStr>],
    args_info: &args_processing::LinkFileInfo,
    cwd: &Path,
    state: &Arc<State>,
    sources: &[PathBuf],
) -> Result<CommandOutput> {
    let task_period = state.task_periods.start(FinalLinkTaskInfo {
        output: args_info.output.clone(),
    });

    let link_args = args_processing::rewrite_to_link_sources(binary, original_args, sources)?;
    let child_output = tokio::process::Command::new(binary.to_standard_binary_name())
        .args(link_args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .current_dir(cwd)
        .spawn()?
        .wait_with_output()
        .await?;
    if !child_output.status.success() {
        return Err(CommandOutput::from_process_output(child_output).into());
    }
    task_period.finished_successfully();
    Ok(CommandOutput::from_process_output(child_output))
}

struct CompileChunkTaskInfo {
    sources: Vec<PathBuf>,
}

impl TaskPeriodInfo for CompileChunkTaskInfo {
    fn category(&self) -> String {
        "Compile".to_string()
    }

    fn terminal_one_liner(&self) -> String {
        self.sources
            .iter()
            .map(|p| shorten_path(p))
            .collect::<Vec<_>>()
            .join(" ")
    }

    fn log_detailed(&self) {
        let mut msg = "Compile chunk: ".to_string();
        for source in &self.sources {
            msg.push_str("  ");
            msg.push_str(&shorten_path(source));
            msg.push('\n');
        }
        log::info!("{}", msg);
    }
}

struct FinalLinkTaskInfo {
    output: PathBuf,
}

impl TaskPeriodInfo for FinalLinkTaskInfo {
    fn category(&self) -> String {
        "Link".to_string()
    }

    fn terminal_one_liner(&self) -> String {
        shorten_path(&self.output)
    }

    fn log_detailed(&self) {
        log::info!("Final link for {}", self.output.to_string_lossy());
    }
}

struct CreateThinArchiveTaskInfo {}

impl TaskPeriodInfo for CreateThinArchiveTaskInfo {
    fn category(&self) -> String {
        "Archive".to_string()
    }

    fn terminal_one_liner(&self) -> String {
        "Create thin archive".to_string()
    }

    fn log_detailed(&self) {
        log::info!("Create thin archive");
    }
}
