#![deny(clippy::unwrap_used)]

use std::{ffi::OsStr, sync::Arc};

use anyhow::Result;
use ccelerate_shared::RunRequestData;

use crate::{
    CommandOutput, State, ar_args,
    database::{FileRecord, store_file_record},
    task_periods::TaskPeriodInfo,
};

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

pub async fn handle_ar_request(
    request: &RunRequestData,
    state: &Arc<State>,
) -> Result<CommandOutput> {
    let request_args_ref: Vec<&OsStr> = request.args.iter().map(|s| s.as_ref()).collect::<Vec<_>>();
    let ar_args = ar_args::BuildStaticArchiveInfo::from_args(&request.cwd, &request_args_ref)?;
    let task_period = state.task_periods.start(BuildStaticArchiveInfo {
        archive_name: ar_args.archive_name.to_string_lossy().to_string(),
    });
    store_file_record(
        &state.conn.lock(),
        &ar_args.archive_path,
        &FileRecord {
            cwd: request.cwd.clone(),
            binary: request.binary,
            args: request.args.clone(),
            local_code_file: None,
            global_includes: None,
            include_defines: None,
            bad_includes: None,
        },
    )?;
    let dummy_archive = crate::ASSETS_DIR
        .get_file("dummy_archive.a")
        .expect("file should exist");
    tokio::fs::write(ar_args.archive_path, dummy_archive.contents()).await?;
    task_period.finished_successfully();
    Ok(CommandOutput::new_ok())
}
