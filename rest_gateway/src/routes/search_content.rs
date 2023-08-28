use actix_web::HttpResponse;

#[tracing::instrument(name = "Search content handler")]
pub async fn search_content() -> HttpResponse {
    HttpResponse::Ok().finish()
}
