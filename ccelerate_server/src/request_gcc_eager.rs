#![deny(clippy::unwrap_used)]

use std::path::Path;

use actix_web::HttpResponse;
use ccelerate_shared::WrappedBinary;
use clap::builder::OsStr;

use crate::{
    State,
    parse_gcc::GCCArgs,
    task_log::{TaskInfo, log_task},
};

struct EagerGccTaskInfo {
    binary: WrappedBinary,
    args: GCCArgs,
}

impl TaskInfo for EagerGccTaskInfo {
    fn short_name(&self) -> String {
        if let Some(output) = self.args.primary_output.as_ref() {
            if let Some(output_name) = output.file_name() {
                return format!("Eager: {}: {}", self.binary, output_name.to_string_lossy());
            }
        }
        let args_str = self
            .args
            .to_args()
            .join(&OsStr::from(" "))
            .to_string_lossy()
            .to_string();
        format!("Eager: {} {}", self.binary, args_str)
    }

    fn log(&self) {
        log::info!("Eager GCC: {:#?}", self.args.to_args());
    }
}

pub async fn handle_eager_gcc_request(
    binary: WrappedBinary,
    request_gcc_args: &GCCArgs,
    cwd: &Path,
    state: &State,
) -> HttpResponse {
    let task_period = log_task(
        &EagerGccTaskInfo {
            binary,
            args: request_gcc_args.clone(),
        },
        state,
    );
    let child = tokio::process::Command::new(binary.to_standard_binary_name())
        .args(request_gcc_args.to_args())
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
