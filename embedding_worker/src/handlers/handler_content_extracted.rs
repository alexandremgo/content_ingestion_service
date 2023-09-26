use std::sync::Arc;

use common::helper::error_chain_fmt;
use futures::StreamExt;

use lapin::{
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
            content_point::{ContentPoint, ContentPointPayload},
            extracted_content::ExtractedContent,
        },
        services::huggingface_embedding::{
            HuggingFaceEmbeddingsService, HuggingFaceEmbeddingsServiceError,
        },
    },
    repositories::{
        content_point_qdrant_repository::{
            ContentPointQdrantRepository, ContentPointQdrantRepositoryError,
        },
        message_rabbitmq_repository::{MessageRabbitMQRepository, MessageRabbitMQRepositoryError},
    },
};

pub const ROUTING_KEY: &str = "content_extracted.v1";

#[derive(thiserror::Error)]
pub enum RegisterHandlerContentExtractedError {
    #[error(transparent)]
    RabbitMQError(#[from] lapin::Error),
    #[error(transparent)]
    MessageRabbitMQRepositoryError(#[from] MessageRabbitMQRepositoryError),
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
        message_rabbitmq_repository,
        content_point_qdrant_repository,
        embeddings_service
    )
)]
pub async fn register_handler(
    rabbitmq_consuming_connection: RabbitMQConnection,
    exchange_name: String,
    // Not an `Arc` shared reference as we want to initialize a new repository for each thread (or at least for each handler)
    mut message_rabbitmq_repository: MessageRabbitMQRepository,
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

    // When supplying an empty string queue name, RabbitMQ generates a name for us, returned from the queue declaration request
    let queue = channel
        .queue_declare("", QueueDeclareOptions::default(), FieldTable::default())
        .await?;

    info!(
        "Declared queue {} on exchange {}, binding on {}",
        queue.name(),
        exchange_name,
        ROUTING_KEY
    );

    channel
        .queue_bind(
            queue.name().as_str(),
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
        .basic_consume(
            queue.name().as_str(),
            "",
            consumer_options,
            FieldTable::default(),
        )
        .await?;

    // Inits for this specific handler
    message_rabbitmq_repository.try_init().await?;

    info!(
        "📡 Handler consuming from queue {}, bound to {} with {}, waiting for messages ...",
        queue.name(),
        exchange_name,
        ROUTING_KEY,
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
                        "Failed to consume queue message on queue {}",
                        queue.name()
                    );
                    return;
                }
            };

            let extracted_content = match ExtractedContent::try_parsing(&delivery.data) {
                Ok(job) => job,
                Err(error) => {
                    error!(
                        ?error,
                        "Failed to parse extracted content message data: {}", error
                    );
                    return;
                }
            };

            info!(?extracted_content, "Received extracted content");

            match execute_handler(
                &mut message_rabbitmq_repository,
                content_point_qdrant_repository.clone(),
                embeddings_service.clone(),
                &extracted_content,
            )
            .await
            {
                Ok(()) => {
                    info!(
                        "Acknowledging message with delivery tag {}",
                        delivery.delivery_tag
                    );
                    if let Err(error) = delivery.ack(BasicAckOptions::default()).await {
                        error!(
                            ?error,
                            ?extracted_content,
                            "Failed to ack extract_content_job message"
                        );
                    }
                }
                Err(error) => {
                    error!(
                        ?error,
                        ?extracted_content,
                        "Failed to handle extract_content_job message"
                    );

                    // TODO: maybe depending on the error we could reject the message and not just nack
                    info!(
                        "Not acknowledging message with delivery tag {}",
                        delivery.delivery_tag
                    );
                    if let Err(error) = delivery.nack(BasicNackOptions::default()).await {
                        error!(
                            ?error,
                            ?extracted_content,
                            "Failed to nack extracted content message"
                        );
                    }
                }
            }
        }
        .instrument(info_span!(
            "Handling consumed message",
            routing_key = ROUTING_KEY,
            exchange = exchange_name,
            queue = %queue.name(),
            message_id = %uuid::Uuid::new_v4(),
        ))
        .await
    }

    Ok(())
}

#[derive(thiserror::Error)]
pub enum ExecuteHandlerContentExtractedError {
    #[error(transparent)]
    MessageRabbitMQRepositoryError(#[from] MessageRabbitMQRepositoryError),
    #[error(transparent)]
    HuggingFaceEmbeddingsServiceError(#[from] HuggingFaceEmbeddingsServiceError),
    #[error(transparent)]
    ContentPointQdrantRepositoryError(#[from] ContentPointQdrantRepositoryError),
}

impl std::fmt::Debug for ExecuteHandlerContentExtractedError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        error_chain_fmt(self, f)
    }
}

#[tracing::instrument(
    name = "Executing handler on extracted content",
    skip(
        _message_rabbitmq_repository,
        content_point_qdrant_repository,
        embeddings_service
    )
)]
pub async fn execute_handler(
    _message_rabbitmq_repository: &mut MessageRabbitMQRepository,
    content_point_qdrant_repository: Arc<ContentPointQdrantRepository>,
    embeddings_service: Arc<HuggingFaceEmbeddingsService>,
    extracted_content: &ExtractedContent,
) -> Result<(), ExecuteHandlerContentExtractedError> {
    let ExtractedContent { content, .. } = extracted_content;

    let embeddings_list = embeddings_service.generate_embeddings(&content).await?;

    // Extracted content for all the generated embeddings from content sentences ?
    let content_points: Vec<ContentPoint> = embeddings_list
        .iter()
        .map(|embeddings| ContentPoint {
            id: Uuid::new_v4(),
            vector: embeddings.to_vec(),
            payload: ContentPointPayload {
                content: content.to_string(),
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
