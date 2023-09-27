use futures::StreamExt;
use lapin::{
    message::Delivery,
    options::{
        BasicAckOptions, BasicConsumeOptions, BasicNackOptions, ExchangeDeclareOptions,
        QueueBindOptions, QueueDeclareOptions,
    },
    types::FieldTable,
    Connection as RabbitMQConnection, ExchangeKind,
};
use std::sync::Arc;
use tracing::{error, info, info_span, Instrument};

use crate::{
    domain::entities::content::ContentEntity,
    repositories::meilisearch_content_repository::{
        MeilisearchContentRepository, MeilisearchContentRepositoryError,
    },
};
use common::{
    constants::routing_keys::CONTENT_EXTRACTED_ROUTING_KEY,
    core::rabbitmq_message_repository::{
        RabbitMQMessageRepository, RabbitMQMessageRepositoryError,
    },
    dtos::extracted_content::ExtractedContentDto,
    helper::error_chain_fmt,
};

pub const ROUTING_KEY: &str = CONTENT_EXTRACTED_ROUTING_KEY;

#[derive(thiserror::Error)]
pub enum RegisterHandlerContentExtractedError {
    #[error(transparent)]
    RabbitMQError(#[from] lapin::Error),
    #[error(transparent)]
    RabbitMQMessageRepositoryError(#[from] RabbitMQMessageRepositoryError),
}

impl std::fmt::Debug for RegisterHandlerContentExtractedError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        error_chain_fmt(self, f)
    }
}

/// Registers the message handler to a given exchange with a specific binding key
///
/// It declares a queue and binds it to the given exchange.
/// It handles messages one by one, there is no handling messages in parallel.
///
/// Some repositories (MessageRabbitMQRepository) are initialized inside the handler
/// to avoid sharing some instances (ex: RabbitMQ channel) between each thread
#[tracing::instrument(
    name = "Register message handler",
    skip(rabbitmq_consuming_connection, message_repository, content_repository)
)]
pub async fn register_handler(
    rabbitmq_consuming_connection: Arc<RabbitMQConnection>,
    exchange_name: String,
    queue_name_prefix: String,
    // Not an `Arc` shared reference as we want to initialize a new repository for each thread (or at least for each handler)
    message_repository: RabbitMQMessageRepository,
    content_repository: Arc<MeilisearchContentRepository>,
) -> Result<(), RegisterHandlerContentExtractedError> {
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

    channel
        .queue_declare(
            &queue_name,
            QueueDeclareOptions::default(),
            FieldTable::default(),
        )
        .await?;

    info!(
        "Declared queue {} on exchange {}, binding on {}",
        queue_name, exchange_name, ROUTING_KEY
    );

    channel
        .queue_bind(
            &queue_name,
            &exchange_name,
            ROUTING_KEY,
            QueueBindOptions::default(),
            FieldTable::default(),
        )
        .await?;

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

            match execute_handler(&message_repository, content_repository.clone(), &delivery).await
            {
                Ok(()) => {
                    info!(
                        "Acknowledging message with delivery tag {}",
                        delivery.delivery_tag
                    );
                    if let Err(error) = delivery.ack(BasicAckOptions::default()).await {
                        error!(?error, "Failed to ack extract_content_job message");
                    }
                }
                Err(error) => {
                    error!(?error, "Failed to handle extract_content_job message");

                    // TODO: maybe depending on the error we could reject the message and not just nack
                    info!(
                        "Not acknowledging message with delivery tag {}",
                        delivery.delivery_tag
                    );
                    if let Err(error) = delivery.nack(BasicNackOptions::default()).await {
                        error!(?error, "Failed to nack extracted content message");
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
    #[error("{0}")]
    MessageParsingError(String),
}

impl std::fmt::Debug for ExecuteHandlerContentExtractedError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        error_chain_fmt(self, f)
    }
}

#[tracing::instrument(
    name = "Executing handler on extracted content",
    skip(message_repository, content_repository, message)
)]
pub async fn execute_handler(
    message_repository: &RabbitMQMessageRepository,
    content_repository: Arc<MeilisearchContentRepository>,
    message: &Delivery,
) -> Result<(), ExecuteHandlerContentExtractedError> {
    let extracted_content = ExtractedContentDto::try_parsing(&message.data).map_err(|error| {
        ExecuteHandlerContentExtractedError::MessageParsingError(format!(
            "Failed to parse extracted content message data: {}",
            error
        ))
    })?;

    info!(?extracted_content, "Received extracted content");
    let content: ContentEntity = extracted_content.into();

    content_repository.save(&content).await?;

    // To inform on progress. Not used currently.
    message_repository
        .publish(
            "content_fulltext_saved.v1",
            serde_json::to_string(&content)?.as_bytes(),
        )
        .await?;

    info!("Successfully handled extract_content_job message");
    Ok(())
}
