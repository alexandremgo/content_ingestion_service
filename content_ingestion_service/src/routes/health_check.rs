use actix_web::HttpResponse;

#[tracing::instrument(name = "Health check handler")]
pub async fn health_check() -> HttpResponse {
    HttpResponse::Ok().finish()
}
