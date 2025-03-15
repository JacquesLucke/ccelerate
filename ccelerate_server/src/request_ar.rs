use std::ffi::OsStr;

use actix_web::{HttpResponse, web::Data};
use ccelerate_shared::RunRequestData;

use crate::{DbFilesRow, DbFilesRowData, State, parse_ar::ArArgs, store_db_file};

pub async fn handle_ar_request(request: &RunRequestData, state: &Data<State>) -> HttpResponse {
    let request_args_ref: Vec<&OsStr> = request.args.iter().map(|s| s.as_ref()).collect::<Vec<_>>();
    let Ok(request_ar_args) = ArArgs::parse(&request.cwd, &request_args_ref) else {
        return HttpResponse::NotImplemented().body("Cannot parse ar arguments");
    };
    let Some(request_output_path) = request_ar_args.output.as_ref() else {
        return HttpResponse::NotImplemented().body("Expected output path");
    };
    let _task_handle = state.tasks_logger.start_task(&format!(
        "Prepare: {}",
        request_output_path.file_name().unwrap().to_string_lossy()
    ));
    store_db_file(
        &state.conn.lock(),
        &DbFilesRow {
            path: request_output_path.clone(),
            data: DbFilesRowData {
                cwd: request.cwd.clone(),
                binary: request.binary,
                args: request_ar_args.to_args(),
                local_code_file: None,
                headers: None,
                global_defines: None,
            },
        },
    )
    .unwrap();
    let dummy_archive = crate::ASSETS_DIR.get_file("dummy_archive.a").unwrap();
    std::fs::write(request_output_path, dummy_archive.contents()).unwrap();
    return HttpResponse::Ok().json(&ccelerate_shared::RunResponseDataWire {
        ..Default::default()
    });
}
