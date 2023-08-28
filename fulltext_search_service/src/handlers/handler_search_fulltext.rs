use futures::StreamExt;
use lapin::{
    options::{
        BasicAckOptions, BasicConsumeOptions, BasicNackOptions, ExchangeDeclareOptions,
        QueueBindOptions, QueueDeclareOptions,
    },
    types::FieldTable,
    Connection as RabbitMQConnection, ExchangeKind,
};
use std::sync::Arc;
use tracing::{error, info, info_span, Instrument};

use crate::repositories::meilisearch_content_repository::{
    MeilisearchContentRepository, MeilisearchContentRepositoryError,
};
use common::{
    constants::routing_keys::SEARCH_FULLTEXT_ROUTING_KEY,
    core::rabbitmq_message_repository::{
        RabbitMQMessageRepository, RabbitMQMessageRepositoryError,
    },
    dtos::{
        fulltext_search_request::FulltextSearchRequestDto,
        fulltext_search_response::{FulltextSearchResponseData, FulltextSearchResponseDto},
        templates::rpc_response::RpcErrorStatus,
    },
    helper::error_chain_fmt,
};

pub const ROUTING_KEY: &str = SEARCH_FULLTEXT_ROUTING_KEY;

#[derive(thiserror::Error)]
pub enum RegisterHandlerSearchFulltextError {
    #[error(transparent)]
    RabbitMQError(#[from] lapin::Error),
    #[error(transparent)]
    RabbitMQMessageRepositoryError(#[from] RabbitMQMessageRepositoryError),
}

impl std::fmt::Debug for RegisterHandlerSearchFulltextError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        error_chain_fmt(self, f)
    }
}

/// Registers the RPC message handler to a given exchange with a specific binding key
///
/// The handler will respond to the message on the given `reply-to`.
///
/// It handles messages one by one, there is no handling messages in parallel.
///
/// Some repositories (MessageRabbitMQRepository) are initialized inside the handler
/// to avoid sharing some instances (ex: RabbitMQ channel) between each thread
#[tracing::instrument(
    name = "Register search fulltext RPC handler",
    skip(rabbitmq_consuming_connection, message_repository, content_repository)
)]
pub async fn register_handler(
    rabbitmq_consuming_connection: Arc<RabbitMQConnection>,
    exchange_name: String,
    queue_name_prefix: String,
    // Not an `Arc` shared reference as we want to initialize a new repository for each thread (or at least for each handler)
    message_repository: RabbitMQMessageRepository,
    content_repository: Arc<MeilisearchContentRepository>,
) -> Result<(), RegisterHandlerSearchFulltextError> {
    let channel = rabbitmq_consuming_connection.create_channel().await?;

    channel
        .exchange_declare(
            &exchange_name,
            ExchangeKind::Topic,
            ExchangeDeclareOptions {
                durable: true,
                ..ExchangeDeclareOptions::default()
            },
            FieldTable::default(),
        )
        .await?;

    // In order to have several nodes of this service as consumers of the same queue: use a specific queue name
    let queue_name = queue_name(&queue_name_prefix);

    let _ = channel
        .queue_declare(
            &queue_name,
            QueueDeclareOptions::default(),
            FieldTable::default(),
        )
        .await?;

    channel
        .queue_bind(
            &queue_name,
            &exchange_name,
            ROUTING_KEY,
            QueueBindOptions::default(),
            FieldTable::default(),
        )
        .await?;

    info!(
        "Declared queue {} on exchange {}, binding on {}",
        queue_name, exchange_name, ROUTING_KEY
    );

    let consumer_options = BasicConsumeOptions {
        no_ack: false,
        ..BasicConsumeOptions::default()
    };

    let mut consumer = channel
        .basic_consume(&queue_name, "", consumer_options, FieldTable::default())
        .await?;

    // Inits for this specific handler
    let message_repository = message_repository.try_init().await?;

    info!(
        "📡 Handler consuming from queue {}, bound to {} with {}, waiting for messages ...",
        queue_name, exchange_name, ROUTING_KEY,
    );

    while let Some(delivery) = consumer.next().await {
        async {
            let delivery = match delivery {
                // Carries the delivery alongside its channel
                Ok(delivery) => delivery,
                // Carries the error and is always followed by Ok(None)
                Err(error) => {
                    error!(
                        ?error,
                        "Failed to consume queue message on queue {}", queue_name
                    );
                    return;
                }
            };

            let reply_to = match delivery.properties.reply_to().as_ref() {
                Some(reply_to) => reply_to,
                None => {
                    error!(
                        "No `reply-to` attribute necessary for RPC call on queue: {}",
                        queue_name
                    );

                    // Disables requeue if there is no way to reply to the RPC call
                    if let Err(error) = delivery
                        .nack(BasicNackOptions {
                            requeue: false,
                            ..BasicNackOptions::default()
                        })
                        .await
                    {
                        error!(?error, "Failed to nack message");
                    }

                    return;
                }
            };

            match execute_handler(
                &message_repository,
                content_repository.clone(),
                &delivery.data,
                reply_to.as_str(),
            )
            .await
            {
                Ok(()) => {
                    info!(
                        "Acknowledging message with delivery tag {}",
                        delivery.delivery_tag
                    );
                    if let Err(error) = delivery.ack(BasicAckOptions::default()).await {
                        error!(?error, "Failed to ack message");
                    }
                }
                Err(error) => {
                    error!(?error, "Failed to handle fulltext search request");

                    // TODO: maps `error` to specific RpcErrorStatus
                    let response = FulltextSearchResponseDto::Error {
                        status: RpcErrorStatus::BadRequest,
                        message: error.to_string(),
                    };

                    if let Ok(response) = FulltextSearchResponseDto::try_serializing(&response) {
                        // Sends response to the given `reply_to` to mimic a RPC call
                        let _ = message_repository
                            .rpc_respond(reply_to.as_str(), response.as_bytes())
                            .await;
                    }

                    info!(
                        "Not acknowledging message with delivery tag {}",
                        delivery.delivery_tag
                    );
                    if let Err(error) = delivery.nack(BasicNackOptions::default()).await {
                        error!(?error, "Failed to nack message");
                    }
                }
            }
        }
        .instrument(info_span!(
            "Handling consumed message",
            routing_key = ROUTING_KEY,
            exchange = exchange_name,
            queue = queue_name,
            message_id = %uuid::Uuid::new_v4(),
        ))
        .await
    }

    Ok(())
}

pub fn queue_name(queue_name_prefix: &str) -> String {
    format!("{}_{}", queue_name_prefix, ROUTING_KEY)
}

#[derive(thiserror::Error)]
pub enum ExecuteHandlerContentExtractedError {
    #[error(transparent)]
    RabbitMQMessageRepositoryError(#[from] RabbitMQMessageRepositoryError),
    #[error(transparent)]
    MeilisearchContentRepositoryError(#[from] MeilisearchContentRepositoryError),
    #[error("Error while serializing message data: {0}")]
    JsonError(#[from] serde_json::Error),
    #[error("Error while deserializing input message: {0}")]
    MessageParsingError(String),
}

impl std::fmt::Debug for ExecuteHandlerContentExtractedError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        error_chain_fmt(self, f)
    }
}

#[tracing::instrument(
    name = "Executing handler on fulltext search request",
    skip(message_repository, content_repository,)
)]
pub async fn execute_handler(
    message_repository: &RabbitMQMessageRepository,
    content_repository: Arc<MeilisearchContentRepository>,
    data: &[u8],
    reply_to: &str,
) -> Result<(), ExecuteHandlerContentExtractedError> {
    let search_request = FulltextSearchRequestDto::try_parsing(data).map_err(|error| {
        ExecuteHandlerContentExtractedError::MessageParsingError(format!(
            "Failed to parse extracted content message data: {}",
            error
        ))
    })?;

    info!(
        ?search_request,
        ?reply_to,
        "Received fulltext search request, executing..."
    );
    let FulltextSearchRequestDto { content, .. } = search_request;

    let content = format!("🦄 Response for {content}");

    let response = FulltextSearchResponseDto::Ok {
        data: FulltextSearchResponseData { content },
    };

    // Sends response to the given `reply_to` to mimic a RPC call
    message_repository
        .rpc_respond(reply_to, serde_json::to_string(&response)?.as_bytes())
        .await?;

    info!("Successfully handled {} message", ROUTING_KEY);
    Ok(())
}
