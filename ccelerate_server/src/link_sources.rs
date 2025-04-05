use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    sync::Arc,
};

use crate::{
    ar_args, args_processing, path_utils::shorten_path, state::State, state_persistent::ObjectData,
    task_periods::TaskPeriodInfo,
};

use anyhow::Result;

#[derive(Debug, Default)]
pub struct OriginalLinkSources {
    // These are link sources that were not compiled here, so they were probably
    // precompiled using a different system.
    pub unknown_sources: Vec<PathBuf>,
    // Those object files are compiled from source here, so we know how they are
    // compiled exactly and can optimize that process.
    pub known_object_files: Vec<ObjectData>,

    handled_paths: HashSet<PathBuf>,
}

pub fn find_link_sources(
    args_info: &args_processing::LinkFileInfo,
    state: &Arc<State>,
) -> Result<OriginalLinkSources> {
    let task_period = state.task_periods.start(FindLinkSourcesTaskInfo {
        output: args_info.output.clone(),
    });

    let mut link_sources = OriginalLinkSources::default();
    for source in args_info.sources.iter() {
        find_link_sources_for_file(&source.path, &mut link_sources, state)?;
    }
    task_period.finished_successfully();
    Ok(link_sources)
}

fn find_link_sources_for_file(
    path: &Path,
    link_sources: &mut OriginalLinkSources,
    state: &Arc<State>,
) -> Result<()> {
    match path.extension() {
        Some(extension) if extension == "a" => {
            find_link_sources_for_static_library(path, link_sources, state)?;
        }
        Some(extension) if extension == "o" => {
            find_link_sources_for_object_file(path, link_sources, state)?;
        }
        _ => {
            link_sources.unknown_sources.push(path.to_owned());
        }
    }
    Ok(())
}

fn find_link_sources_for_static_library(
    library_path: &Path,
    link_sources: &mut OriginalLinkSources,
    state: &Arc<State>,
) -> Result<()> {
    if !link_sources.handled_paths.insert(library_path.to_owned()) {
        return Ok(());
    }
    let Some(record) = state.persistent.get_archive_file(library_path) else {
        link_sources.unknown_sources.push(library_path.to_owned());
        return Ok(());
    };
    if !record.binary.is_ar_compatible() {
        return Err(anyhow::anyhow!(
            "Archive not created by ar: {}",
            library_path.display()
        ));
    }
    let ar_args = ar_args::BuildStaticArchiveInfo::from_args(&record.cwd, &record.args)?;
    for source in ar_args.member_paths {
        find_link_sources_for_file(&source, link_sources, state)?;
    }
    Ok(())
}

fn find_link_sources_for_object_file(
    object_path: &Path,
    link_sources: &mut OriginalLinkSources,
    state: &Arc<State>,
) -> Result<()> {
    if !link_sources.handled_paths.insert(object_path.to_owned()) {
        return Ok(());
    }
    let Some(record) = state.persistent.get_object_file(object_path) else {
        link_sources.unknown_sources.push(object_path.to_owned());
        return Ok(());
    };
    if !record.create.binary.is_gcc_compatible() {
        return Err(anyhow::anyhow!(
            "Object file not created by gcc compatible: {}",
            object_path.display()
        ));
    }
    link_sources.known_object_files.push(record);
    Ok(())
}

struct FindLinkSourcesTaskInfo {
    output: PathBuf,
}

impl TaskPeriodInfo for FindLinkSourcesTaskInfo {
    fn category(&self) -> String {
        "Find Sources".to_string()
    }

    fn terminal_one_liner(&self) -> String {
        shorten_path(&self.output)
    }

    fn log_detailed(&self) {
        log::info!("Find link sources for {}", self.output.to_string_lossy());
    }
}
