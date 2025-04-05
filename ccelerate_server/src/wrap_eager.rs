#![deny(clippy::unwrap_used)]

use std::{
    ffi::{OsStr, OsString},
    path::Path,
};

use anyhow::Result;
use ccelerate_shared::WrappedBinary;

use crate::{CommandOutput, State, task_periods::TaskPeriodInfo};

pub async fn wrap_eager(
    binary: WrappedBinary,
    args: &[impl AsRef<OsStr>],
    cwd: &Path,
    state: &State,
) -> Result<CommandOutput> {
    let task_period = state.task_periods.start(EagerGccTaskInfo {
        binary,
        args: args.iter().map(|s| s.as_ref().to_owned()).collect(),
    });
    let child = tokio::process::Command::new(binary.to_standard_binary_name())
        .args(args)
        .current_dir(cwd)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()?;
    let child_result = child.wait_with_output().await?;
    task_period.finished_successfully();
    Ok(CommandOutput::from_process_output(child_result))
}

struct EagerGccTaskInfo {
    binary: WrappedBinary,
    args: Vec<OsString>,
}

impl TaskPeriodInfo for EagerGccTaskInfo {
    fn category(&self) -> String {
        "Eager".to_string()
    }

    fn terminal_one_liner(&self) -> String {
        format!("{} {:?}", self.binary, self.args)
    }

    fn log_detailed(&self) {
        log::info!("{} {:?}", self.binary, self.args);
    }
}
