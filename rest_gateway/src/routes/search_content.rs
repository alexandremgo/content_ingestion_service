use actix_web::http::StatusCode;
use actix_web::{web, HttpResponse, ResponseError};
use common::core::rabbitmq_message_repository::RabbitMQMessageRepositoryError;
use common::dtos::fulltext_search_response::FulltextSearchResponseDto;
use common::dtos::templates::rpc_response::RpcResponseEncodingError;
use common::{
    constants::routing_keys::SEARCH_FULLTEXT_ROUTING_KEY,
    core::rabbitmq_message_repository::RabbitMQMessageRepository,
    dtos::fulltext_search_request::{FulltextSearchRequestDto, FulltextSearchRequestDtoError},
    helper::error_chain_fmt,
};
use serde_json::Value as JsonValue;
use tracing::info;

#[tracing::instrument(name = "Search content handler", skip(message_rabbitmq_repository))]
pub async fn search_content(
    message_rabbitmq_repository: web::Data<RabbitMQMessageRepository>,
    body: web::Json<BodyData>,
) -> Result<HttpResponse, SearchContentError> {
    info!("Searching contents for query: {}", body.query);

    let request = FulltextSearchRequestDto {
        metadata: JsonValue::Null,
        query: body.query.clone(),
        limit: body.limit,
    };
    let request = request.try_serializing()?;

    let response = message_rabbitmq_repository
        .rpc_call(SEARCH_FULLTEXT_ROUTING_KEY, request.as_bytes(), None)
        .await?;

    let response = FulltextSearchResponseDto::try_parsing(&response)?;

    Ok(HttpResponse::Ok().json(response))
}

#[derive(Debug, serde::Deserialize)]
pub struct BodyData {
    query: String,
    limit: Option<usize>,
}

#[derive(thiserror::Error)]
pub enum SearchContentError {
    #[error("Error while publishing messages on RabbitMQ broker: {0}")]
    RabbitMQMessageRepositoryError(#[from] RabbitMQMessageRepositoryError),
    #[error("Error while generation full-text search internal request: {0}")]
    FulltextSearchRequestError(#[from] FulltextSearchRequestDtoError),
    #[error("Error while parsing response: {0}")]
    RpcResponseEncodingError(#[from] RpcResponseEncodingError),
}

impl std::fmt::Debug for SearchContentError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        error_chain_fmt(self, f)
    }
}

impl ResponseError for SearchContentError {
    fn status_code(&self) -> StatusCode {
        match self {
            SearchContentError::FulltextSearchRequestError(_)
            | SearchContentError::RpcResponseEncodingError(_)
            | SearchContentError::RabbitMQMessageRepositoryError(_) => {
                StatusCode::INTERNAL_SERVER_ERROR
            } // AddSourceFilesError::NoSourceFiles => StatusCode::BAD_REQUEST,
        }
    }
}
