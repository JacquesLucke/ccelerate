#![deny(clippy::unwrap_used)]

use std::{ffi::OsStr, path::Path, sync::Arc};

use anyhow::Result;
use ccelerate_shared::WrappedBinary;

use crate::{CommandOutput, State, ar_args, task_periods::TaskPeriodInfo};

struct BuildStaticArchiveInfo {
    archive_name: String,
}

impl TaskPeriodInfo for BuildStaticArchiveInfo {
    fn category(&self) -> String {
        "Ar".to_string()
    }

    fn terminal_one_liner(&self) -> String {
        self.archive_name.clone()
    }

    fn log_detailed(&self) {
        log::info!("Prepare: {}", self.archive_name);
    }
}

pub async fn wrap_create_static_archive(
    binary: WrappedBinary,
    args: &[impl AsRef<OsStr>],
    cwd: &Path,
    state: &Arc<State>,
) -> Result<CommandOutput> {
    let ar_args = ar_args::BuildStaticArchiveInfo::from_args(cwd, args)?;
    let task_period = state.task_periods.start(BuildStaticArchiveInfo {
        archive_name: ar_args.archive_name.to_string_lossy().to_string(),
    });
    state
        .persistent
        .update_archive_file(&ar_args.archive_path, binary, cwd, args)?;

    let dummy_archive = crate::ASSETS_DIR
        .get_file("dummy_archive.a")
        .expect("file should exist");
    tokio::fs::write(ar_args.archive_path, dummy_archive.contents()).await?;
    task_period.finished_successfully();
    Ok(CommandOutput::new_ok())
}
