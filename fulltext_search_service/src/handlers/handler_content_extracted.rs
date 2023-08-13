use std::sync::Arc;

use common::{
    core::rabbitmq_message_repository::{
        RabbitMQMessageRepository, RabbitMQMessageRepositoryError,
    },
    dtos::extracted_content::ExtractedContentDto,
    helper::error_chain_fmt,
};
use futures::StreamExt;

use lapin::{
    options::{
        BasicAckOptions, BasicConsumeOptions, BasicNackOptions, ExchangeDeclareOptions,
        QueueBindOptions, QueueDeclareOptions,
    },
    types::FieldTable,
    Connection as RabbitMQConnection, ExchangeKind,
};
use tracing::{debug, error, info, info_span, Instrument};
use uuid::Uuid;

use crate::repositories::meilisearch_content_repository::MeilisearchContentRepository;

pub const ROUTING_KEY: &str = "content_extracted.v1";

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
    rabbitmq_consuming_connection: RabbitMQConnection,
    exchange_name: String,
    queue_name_prefix: String,
    // Not an `Arc` shared reference as we want to initialize a new repository for each thread (or at least for each handler)
    mut message_repository: RabbitMQMessageRepository,
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

    // In order to have several nodes of this service as consumers of the same queues
    let queue_name = format!("{}_{}", queue_name_prefix, ROUTING_KEY);

    let _ = channel
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
    message_repository.try_init().await?;

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

            let extracted_content = match ExtractedContentDto::try_parsing(&delivery.data) {
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
                &mut message_repository,
                content_repository.clone(),
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
            queue = queue_name,
            message_id = %uuid::Uuid::new_v4(),
        ))
        .await
    }

    Ok(())
}

#[derive(thiserror::Error)]
pub enum ExecuteHandlerContentExtractedError {
    #[error(transparent)]
    RabbitMQMessageRepositoryError(#[from] RabbitMQMessageRepositoryError),
}

impl std::fmt::Debug for ExecuteHandlerContentExtractedError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        error_chain_fmt(self, f)
    }
}

#[tracing::instrument(
    name = "Executing handler on extracted content",
    skip(message_repository, content_repository,)
)]
pub async fn execute_handler(
    message_repository: &mut RabbitMQMessageRepository,
    content_repository: Arc<MeilisearchContentRepository>,
    extracted_content: &ExtractedContentDto,
) -> Result<(), ExecuteHandlerContentExtractedError> {
    let ExtractedContentDto {
        metadata, content, ..
    } = extracted_content;

    // // Extracted content for all the generated embeddings from content sentences ?
    // let content_points: Vec<ContentPoint> = embeddings_list
    //     .iter()
    //     .map(|embeddings| ContentPoint {
    //         id: Uuid::new_v4(),
    //         vector: embeddings.to_vec(),
    //         payload: ContentPointPayload {
    //             content: content.to_string(),
    //         },
    //     })
    //     .collect();

    // info!(?content_points, "Generated embeddings");

    // content_point_qdrant_repository
    //     .batch_save(content_points)
    //     .await?;

    info!("Successfully handled extract_content_job message");
    Ok(())
}
