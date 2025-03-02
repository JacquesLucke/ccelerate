use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use actix_web::HttpResponse;
use anyhow::Result;
use base64::prelude::*;
use ccelerate_shared::RunRequestData;
use parking_lot::Mutex;
use rusqlite_migration::{M, Migrations};

use path_utils::make_absolute;

mod parse_ar;
mod parse_gcc;
mod path_utils;

struct State {
    conn: Arc<Mutex<rusqlite::Connection>>,
}

#[derive(Debug)]
struct FileToObjectFileCommand {
    cwd: PathBuf,
    binary: String,
    args: Vec<String>,
    input: PathBuf,
    output: PathBuf,
}

#[derive(Debug)]
struct ArchiveCommand {
    cwd: PathBuf,
    binary: String,
    args: Vec<String>,
    input_libs: Vec<PathBuf>,
    input_objects: Vec<PathBuf>,
    output: PathBuf,
}

#[derive(Debug)]
struct BinaryCommand {
    cwd: PathBuf,
    binary: String,
    args: Vec<String>,
    inputs: Vec<PathBuf>,
    output: PathBuf,
}

#[derive(Debug)]
struct UnknownCommand {
    _cwd: PathBuf,
    _binary: String,
    _args: Vec<String>,
}

#[derive(Debug)]
enum Command {
    FileToOjectFile(FileToObjectFileCommand),
    Archive(ArchiveCommand),
    Binary(BinaryCommand),
    Unknown(UnknownCommand),
}

fn parse_command(run_request: &RunRequestData) -> Command {
    if ["gcc", "g++", "clang", "clang++"].contains(&run_request.binary.as_str()) {
        if run_request.args.iter().any(|arg| arg == "-c") {
            if let Some(out_pos) = run_request.args.iter().position(|arg| arg == "-o") {
                if let Some(out_arg) = &run_request.args.get(out_pos + 1) {
                    let output = make_absolute(&run_request.cwd, Path::new(out_arg));
                    let input = make_absolute(
                        &run_request.cwd,
                        Path::new(run_request.args.last().unwrap()),
                    );
                    return Command::FileToOjectFile(FileToObjectFileCommand {
                        cwd: run_request.cwd.clone(),
                        binary: run_request.binary.clone(),
                        args: run_request.args.clone(),
                        input: input.clone(),
                        output,
                    });
                }
            }
        } else {
            if let Some(out_pos) = run_request.args.iter().position(|arg| arg == "-o") {
                if let Some(out_arg) = &run_request.args.get(out_pos + 1) {
                    let output = make_absolute(&run_request.cwd, Path::new(out_arg));
                    let inputs = run_request
                        .args
                        .iter()
                        .filter(|a| {
                            a.ends_with(".o")
                                || a.ends_with(".a")
                                || a.ends_with(".so")
                                || a.ends_with(".c")
                                || a.ends_with(".cc")
                                || a.ends_with(".cpp")
                                || a.ends_with(".cxx")
                        })
                        .map(|a| make_absolute(&run_request.cwd, Path::new(a)))
                        .collect();
                    return Command::Binary(BinaryCommand {
                        cwd: run_request.cwd.clone(),
                        binary: run_request.binary.clone(),
                        args: run_request.args.clone(),
                        inputs: inputs,
                        output,
                    });
                }
            }
        }
    }
    if run_request.binary == "ar" {
        let libs: Vec<PathBuf> = run_request
            .args
            .iter()
            .filter(|a| a.ends_with(".a"))
            .map(|a| PathBuf::from(a))
            .collect();
        let output_lib = libs[0].clone();
        let inputs_libs: Vec<PathBuf> = libs[1..].to_vec();
        let inputs_objects: Vec<PathBuf> = run_request
            .args
            .iter()
            .filter(|a| a.ends_with(".o"))
            .map(|a| PathBuf::from(a))
            .collect();
        return Command::Archive(ArchiveCommand {
            cwd: run_request.cwd.clone(),
            binary: run_request.binary.clone(),
            args: run_request.args.clone(),
            input_libs: inputs_libs,
            input_objects: inputs_objects,
            output: output_lib,
        });
    }
    Command::Unknown(UnknownCommand {
        _cwd: run_request.cwd.clone(),
        _binary: run_request.binary.clone(),
        _args: run_request.args.clone(),
    })
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
    run_request: actix_web::web::Json<RunRequestData>,
    state: actix_web::web::Data<State>,
) -> impl actix_web::Responder {
    let eager_evaluation = need_eager_evaluation(&run_request);

    let parsed_command = parse_command(&run_request);
    match parsed_command {
        Command::FileToOjectFile(command) => {
            let conn = state.conn.lock();
            conn.execute(
                "INSERT OR REPLACE INTO ObjectFiles (output_path, input_path, cwd, binary, args) VALUES (?1, ?2, ?3, ?4, ?5)",
                rusqlite::params![
                    command.output.to_str().unwrap(),
                    command.input.to_str().unwrap(),
                    command.cwd.to_str().unwrap(),
                    command.binary,
                    serde_json::to_string(&command.args).unwrap()
                ],
            )
            .unwrap();
        }
        Command::Archive(command) => {
            let conn = state.conn.lock();
            conn.execute(
                "INSERT OR REPLACE INTO ArchiveFiles (output_path, binary, args, cwd, input_libs, input_objects) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                rusqlite::params![
                    command.output.to_str().unwrap(),
                    command.binary,
                    serde_json::to_string(&command.args).unwrap(),
                    command.cwd.to_str().unwrap(),
                    serde_json::to_string(&command.input_libs).unwrap(),
                    serde_json::to_string(&command.input_objects).unwrap(),
                ],
            )
            .unwrap();
        }
        Command::Binary(command) => {
            let conn = state.conn.lock();
            conn.execute(
                "INSERT OR REPLACE INTO BinaryFiles (output_path, binary, args, cwd, inputs) VALUES (?1, ?2, ?3, ?4, ?5)",
                rusqlite::params![
                    command.output.to_str().unwrap(),
                    command.binary,
                    serde_json::to_string(&command.args).unwrap(),
                    command.cwd.to_str().unwrap(),
                    serde_json::to_string(&command.inputs).unwrap(),
                ],
            )
            .unwrap();
        }
        Command::Unknown(unknown) => {
            println!("{:?}", unknown);
        }
    }

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
    let addr = "127.0.0.1:6235";
    println!("Listening on http://{}", addr);

    let db_migrations = Migrations::new(vec![
        M::up(
            "CREATE TABLE ObjectFiles(
                output_path TEXT NOT NULL PRIMARY KEY,
                input_path TEXT NOT NULL,
                cwd TEXT NOT NULL,
                binary TEXT NOT NULL,
                args JSON NOT NULL
            );",
        ),
        M::up(
            "CREATE TABLE ArchiveFiles(
                output_path TEXT NOT NULL PRIMARY KEY,
                binary TEXT NOT NULL,
                args JSON NOT NULL,
                cwd TEXT NOT NULL,
                input_libs JSON NOT NULL,
                input_objects JSON NOT NULL
            );",
        ),
        M::up(
            "CREATE TABLE BinaryFiles(
                output_path TEXT NOT NULL PRIMARY KEY,
                binary TEXT NOT NULL,
                args JSON NOT NULL,
                cwd TEXT NOT NULL,
                inputs JSON NOT NULL
            );",
        ),
    ]);

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
