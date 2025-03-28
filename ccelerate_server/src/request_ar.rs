#![deny(clippy::unwrap_used)]

use std::ffi::OsStr;

use actix_web::{HttpResponse, web::Data};
use ccelerate_shared::RunRequestData;

use crate::{
    State,
    database::{FileRecord, store_file_record},
    parse_ar::ArArgs,
};

pub async fn handle_ar_request(request: &RunRequestData, state: &Data<State>) -> HttpResponse {
    let request_args_ref: Vec<&OsStr> = request.args.iter().map(|s| s.as_ref()).collect::<Vec<_>>();
    let Ok(request_ar_args) = ArArgs::parse(&request.cwd, &request_args_ref) else {
        return HttpResponse::BadRequest().body("Cannot parse ar arguments");
    };
    let Some(request_output_path) = request_ar_args.output.as_ref() else {
        return HttpResponse::BadRequest().body("Expected output path");
    };
    let Some(output_file_name) = request_output_path.file_name() else {
        return HttpResponse::BadRequest().body("Expected output file name");
    };
    let task_period = state.task_periods.start(
        "Ar",
        &format!("Prepare: {}", output_file_name.to_string_lossy()),
    );
    let Ok(_) = store_file_record(
        &state.conn.lock(),
        request_output_path,
        &FileRecord {
            cwd: request.cwd.clone(),
            binary: request.binary,
            args: request_ar_args.to_args(),
            local_code_file: None,
            global_includes: None,
            include_defines: None,
            bad_includes: None,
        },
    ) else {
        return HttpResponse::InternalServerError().body("Failed to store db file");
    };
    let Some(dummy_archive) = crate::ASSETS_DIR.get_file("dummy_archive.a") else {
        return HttpResponse::InternalServerError().body("Failed to get dummy archive");
    };
    let Ok(_) = std::fs::write(request_output_path, dummy_archive.contents()) else {
        return HttpResponse::InternalServerError().body("Failed to write dummy archive");
    };
    task_period.finished_successfully();
    HttpResponse::Ok().json(ccelerate_shared::RunResponseDataWire {
        ..Default::default()
    })
}
