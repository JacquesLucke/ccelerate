use actix_web::HttpResponse;
use anyhow::Result;

#[actix_web::get("/")]
async fn route_index() -> impl actix_web::Responder {
    "ccelerator".to_string()
}

#[actix_web::post("/run")]
async fn route_run(
    run_request: actix_web::web::Json<ccelerate_shared::RunRequestData>,
) -> impl actix_web::Responder {
    println!("{:?}", run_request);
    HttpResponse::Ok().body("Done")
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
