#![deny(clippy::unwrap_used)]

use std::{
    ffi::{OsStr, OsString},
    path::Path,
};

use actix_web::HttpResponse;
use ccelerate_shared::WrappedBinary;

use crate::{State, task_periods::TaskPeriodInfo};

struct EagerGccTaskInfo {
    binary: WrappedBinary,
    args: Vec<OsString>,
}

impl TaskPeriodInfo for EagerGccTaskInfo {
    fn category(&self) -> String {
        "Eager".to_string()
    }

    fn short_name(&self) -> String {
        format!("{} {:?}", self.binary, self.args)
    }

    fn log(&self) {
        log::info!("{} {:?}", self.binary, self.args);
    }
}

pub async fn handle_eager_gcc_request<S: AsRef<OsStr>>(
    binary: WrappedBinary,
    args: &[S],
    cwd: &Path,
    state: &State,
) -> HttpResponse {
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
        .spawn();
    let Ok(child) = child else {
        return HttpResponse::InternalServerError().body("Failed to spawn child");
    };
    let Ok(child_result) = child.wait_with_output().await else {
        return HttpResponse::InternalServerError().body("Failed to wait on child");
    };
    task_period.finished_successfully();
    HttpResponse::Ok().json(
        ccelerate_shared::RunResponseData {
            stdout: child_result.stdout,
            stderr: child_result.stderr,
            status: child_result.status.code().unwrap_or(1),
        }
        .to_wire(),
    )
}
