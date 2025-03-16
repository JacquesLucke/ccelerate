#![deny(clippy::unwrap_used)]

use std::path::Path;

use actix_web::HttpResponse;
use ccelerate_shared::WrappedBinary;

use crate::{State, parse_gcc::GCCArgs};

pub async fn handle_eager_gcc_request(
    binary: WrappedBinary,
    request_gcc_args: &GCCArgs,
    cwd: &Path,
    state: &State,
) -> HttpResponse {
    let _log_handle = state.tasks_logger.start_task(&format!(
        "Eager: {:?} {}",
        binary.to_standard_binary_name(),
        request_gcc_args
            .to_args()
            .iter()
            .map(|s| s.to_string_lossy())
            .collect::<Vec<_>>()
            .join(" ")
    ));
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
    HttpResponse::Ok().json(
        ccelerate_shared::RunResponseData {
            stdout: child_result.stdout,
            stderr: child_result.stderr,
            status: child_result.status.code().unwrap_or(1),
        }
        .to_wire(),
    )
}
