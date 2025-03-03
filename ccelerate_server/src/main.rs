use std::{ffi::OsStr, sync::Arc};

use actix_web::HttpResponse;
use anyhow::Result;
use base64::prelude::*;
use ccelerate_shared::RunRequestData;
use parking_lot::Mutex;
use rusqlite_migration::{M, Migrations};

mod parse_ar;
mod parse_gcc;
mod path_utils;

struct State {
    conn: Arc<Mutex<rusqlite::Connection>>,
}

#[actix_web::get("/")]
async fn route_index() -> impl actix_web::Responder {
    "ccelerator".to_string()
}

fn need_eager_evaluation(run_request: &RunRequestData) -> bool {
    let marker = "CMakeScratch";
    if run_request.cwd.to_str().unwrap_or("").contains(marker) {
        return true;
    }
    for arg in &run_request.args {
        if arg.contains(marker) {
            return true;
        }
    }
    return false;
}

#[actix_web::post("/run")]
async fn route_run(
    mut run_request: actix_web::web::Json<RunRequestData>,
    state: actix_web::web::Data<State>,
) -> impl actix_web::Responder {
    let eager_evaluation = need_eager_evaluation(&run_request);

    let output_path = match run_request.binary.as_str() {
        "ar" => {
            if let Ok(args) = parse_ar::ArArgs::parse(
                &run_request.cwd,
                run_request
                    .args
                    .iter()
                    .map(|s| OsStr::new(s))
                    .collect::<Vec<_>>()
                    .as_slice(),
            ) {
                run_request.args = args
                    .to_args()
                    .iter()
                    .map(|s| s.to_string_lossy().to_string())
                    .collect();
                args.output
            } else {
                eprintln!("Failed: {:#?}", run_request);
                std::process::exit(1);
            }
        }
        "gcc" | "g++" | "clang" | "clang++" => {
            if let Ok(args) = parse_gcc::GCCArgs::parse(
                &run_request.cwd,
                run_request
                    .args
                    .iter()
                    .map(|s| OsStr::new(s))
                    .collect::<Vec<_>>()
                    .as_slice(),
            ) {
                run_request.args = args
                    .to_args()
                    .iter()
                    .map(|s| s.to_string_lossy().to_string())
                    .collect();
                args.primary_output
            } else {
                eprintln!("Failed: {:#?}", run_request);
                std::process::exit(1);
            }
        }
        _ => {
            eprintln!("Failed: {:#?}", run_request);
            std::process::exit(1);
        }
    };

    if let Some(output_path) = output_path {
        let conn = state.conn.lock();
        conn.execute(
            "INSERT OR REPLACE INTO Files (binary, path, cwd, args) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![
                run_request.binary,
                output_path.to_string_lossy(),
                run_request.cwd.to_str().unwrap(),
                serde_json::to_string(&run_request.args).unwrap()
            ],
        )
        .unwrap();
    }

    println!("{:#?}", run_request);

    let Ok(command) = tokio::process::Command::new(&run_request.binary)
        .args(&run_request.args)
        .current_dir(&run_request.cwd)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
    else {
        return HttpResponse::InternalServerError().body("Failed to spawn command");
    };
    let result = command.wait_with_output().await;
    let result = match result {
        Ok(result) => result,
        Err(err) => {
            return HttpResponse::InternalServerError().body(format!("{}", err));
        }
    };
    let response_data = ccelerate_shared::RunResponseData {
        stdout: if eager_evaluation {
            BASE64_STANDARD.encode(&result.stdout)
        } else {
            "".to_string()
        },
        stderr: if eager_evaluation {
            BASE64_STANDARD.encode(&result.stderr)
        } else {
            "".to_string()
        },
        status: result.status.code().unwrap_or(1),
    };
    HttpResponse::Ok().json(&response_data)
}

#[tokio::main]
async fn main() -> Result<()> {
    let addr = format!("127.0.0.1:{}", ccelerate_shared::DEFAULT_PORT);
    println!("Listening on http://{}", addr);

    let db_migrations = Migrations::new(vec![M::up(
        "CREATE TABLE Files(
                path TEXT NOT NULL PRIMARY KEY,
                cwd TEXT NOT NULL,
                binary TEXT NOT NULL,
                args JSON NOT NULL
            );",
    )]);

    let db_path = "./ccelerate.db";
    let mut conn = rusqlite::Connection::open(db_path)?;
    db_migrations.to_latest(&mut conn)?;

    let state = actix_web::web::Data::new(State {
        conn: Arc::new(Mutex::new(conn)),
    });

    actix_web::HttpServer::new(move || {
        actix_web::App::new()
            .app_data(state.clone())
            .service(route_index)
            .service(route_run)
    })
    .bind(addr)?
    .run()
    .await?;
    Ok(())
}
