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

use crate::{
    domain::entities::extracted_content::ExtractedContent,
    helper::error_chain_fmt,
    repositories::message_rabbitmq_repository::{
        MessageRabbitMQRepository, MessageRabbitMQRepositoryError,
    },
};

pub const BINDING_KEY: &str = "content_extracted.v1";

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
    skip(rabbitmq_consuming_connection, message_rabbitmq_repository)
)]
pub async fn register_handler(
    rabbitmq_consuming_connection: RabbitMQConnection,
    exchange_name: String,
    // Not an `Arc` shared reference as we want to initialize a new repository for each thread (or at least for each handler)
    mut message_rabbitmq_repository: MessageRabbitMQRepository,
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
        BINDING_KEY
    );

    channel
        .queue_bind(
            queue.name().as_str(),
            &exchange_name,
            BINDING_KEY,
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

    // One (for publishing) channel for this collection of handlers
    message_rabbitmq_repository.try_init().await?;
    // The fact that we need a collection-wide mutex like this is hinting that there is a problem
    // Each spawned handler will have to lock this repository to be able to publish.
    // At least this will limit the usage of the same channel in parallel.
    // But another solution should be preferable.
    // Maybe a pool of channels that are behind mutexes and can be reset when needed because one failed ?
    // Then we limit the number of parallel handlers to the number of available channels.
    // Or is it better to fail fast and re-start a worker ?
    // let message_rabbitmq_repository = Arc::new(Mutex::new(message_rabbitmq_repository));

    info!(
        "📡 Handler consuming from queue {}, bound to {} with {}, waiting for messages ...",
        queue.name(),
        exchange_name,
        BINDING_KEY,
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

            match execute_handler(&mut message_rabbitmq_repository, &extracted_content).await {
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
            binding_key = BINDING_KEY,
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
}

impl std::fmt::Debug for ExecuteHandlerContentExtractedError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        error_chain_fmt(self, f)
    }
}

#[tracing::instrument(
    name = "Executing handler on extracted content",
    skip(message_rabbitmq_repository)
)]
pub async fn execute_handler(
    message_rabbitmq_repository: &mut MessageRabbitMQRepository,
    extracted_content: &ExtractedContent,
) -> Result<(), ExecuteHandlerContentExtractedError> {
    let ExtractedContent {
        metadata, content, ..
    } = extracted_content;

    info!(?metadata, ?content, "Executing handler");

    Ok(())
}
