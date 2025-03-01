use actix_web::HttpResponse;
use anyhow::Result;
use base64::prelude::*;
use ccelerate_shared::RunRequestData;

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
async fn route_run(run_request: actix_web::web::Json<RunRequestData>) -> impl actix_web::Responder {
    println!("{:?}", run_request);
    let eager_evaluation = need_eager_evaluation(&run_request);
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
    actix_web::HttpServer::new(|| {
        actix_web::App::new()
            .service(route_index)
            .service(route_run)
    })
    .bind(addr)?
    .run()
    .await?;
    Ok(())
}
