use std::sync::Arc;

use common::{
    constants::routing_keys::CONTENT_EXTRACTED_ROUTING_KEY,
    core::rabbitmq_message_repository::{
        RabbitMQMessageRepository, RabbitMQMessageRepositoryError,
    },
    dtos::extracted_content::ExtractedContentDto,
    helper::error_chain_fmt,
};
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
use tracing::{error, info, info_span, Instrument};
use uuid::Uuid;

use crate::{
    domain::{
        entities::{
            content::ContentEntity,
            content_point::{ContentPoint, ContentPointPayload},
        },
        services::huggingface_embedding::{
            HuggingFaceEmbeddingsService, HuggingFaceEmbeddingsServiceError,
        },
    },
    repositories::content_point_qdrant_repository::{
        ContentPointQdrantRepository, ContentPointQdrantRepositoryError,
    },
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
    skip(
        rabbitmq_consuming_connection,
        message_repository,
        content_point_qdrant_repository,
        embeddings_service
    )
)]
pub async fn register_handler(
    rabbitmq_consuming_connection: RabbitMQConnection,
    exchange_name: String,
    queue_name_prefix: String,
    // Not an `Arc` shared reference as we want to initialize a new repository for each thread (or at least for each handler)
    message_repository: RabbitMQMessageRepository,
    content_point_qdrant_repository: Arc<ContentPointQdrantRepository>,
    embeddings_service: Arc<HuggingFaceEmbeddingsService>,
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

            // let extracted_content = match ExtractedContent::try_parsing(&delivery.data) {
            //     Ok(job) => job,
            //     Err(error) => {
            //         error!(
            //             ?error,
            //             "Failed to parse extracted content message data: {}", error
            //         );
            //         return;
            //     }
            // };

            // info!(?extracted_content, "Received extracted content");

            match execute_handler(
                &message_repository,
                content_point_qdrant_repository.clone(),
                embeddings_service.clone(),
                &delivery,
            )
            .await
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
    HuggingFaceEmbeddingsServiceError(#[from] HuggingFaceEmbeddingsServiceError),
    #[error(transparent)]
    ContentPointQdrantRepositoryError(#[from] ContentPointQdrantRepositoryError),
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
    skip(
        _message_repository,
        content_point_qdrant_repository,
        embeddings_service
    )
)]
pub async fn execute_handler(
    _message_repository: &RabbitMQMessageRepository,
    content_point_qdrant_repository: Arc<ContentPointQdrantRepository>,
    embeddings_service: Arc<HuggingFaceEmbeddingsService>,
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

    let embeddings_list = embeddings_service
        .generate_embeddings(&content.content)
        .await?;

    // Extracted content for all the generated embeddings from content sentences ?
    let content_points: Vec<ContentPoint> = embeddings_list
        .iter()
        .map(|embeddings| ContentPoint {
            id: Uuid::new_v4(),
            vector: embeddings.to_vec(),
            payload: ContentPointPayload {
                content: content.content.to_string(),
            },
        })
        .collect();

    info!(?content_points, "Generated embeddings");

    content_point_qdrant_repository
        .batch_save(content_points)
        .await?;

    info!("Successfully handled extract_content_job message");
    Ok(())
}
