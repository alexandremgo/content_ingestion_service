use futures::StreamExt;
use std::{io::Cursor, sync::Arc};

use genawaiter::GeneratorState;
use lapin::{
    options::{
        BasicAckOptions, BasicConsumeOptions, BasicNackOptions, ExchangeDeclareOptions,
        QueueBindOptions, QueueDeclareOptions,
    },
    types::FieldTable,
    Connection as RabbitMQConnection, ExchangeKind,
};
use serde_json::json;
use tracing::{error, info, info_span, Instrument};

use crate::{
    domain::{
        entities::{epub_reader::EpubReader, extract_content_job::ExtractContentJob, xml_reader},
        services::extract_content_generator::extract_content_generator,
    },
    helper::error_chain_fmt,
    repositories::{
        extracted_content_meilisearch_repository::{
            ExtractedContentMeilisearchRepository, ExtractedContentMeilisearchRepositoryError,
        },
        message_rabbitmq_repository::{MessageRabbitMQRepository, MessageRabbitMQRepositoryError},
        source_file_s3_repository::{S3Repository, S3RepositoryError},
    },
};

pub const BINDING_KEY: &str = "extract_content.text.v1";

#[derive(thiserror::Error)]
pub enum RegisterHandlerExtractContentJobError {
    #[error(transparent)]
    RabbitMQError(#[from] lapin::Error),
    #[error(transparent)]
    MessageRabbitMQRepositoryError(#[from] MessageRabbitMQRepositoryError),
}

impl std::fmt::Debug for RegisterHandlerExtractContentJobError {
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
        s3_repository,
        extracted_content_meilisearch_repository,
        message_rabbitmq_repository
    )
)]
pub async fn register_handler(
    rabbitmq_consuming_connection: RabbitMQConnection,
    exchange_name: String,
    s3_repository: Arc<S3Repository>,
    extracted_content_meilisearch_repository: Arc<ExtractedContentMeilisearchRepository>,
    // Not an `Arc` shared reference as we want to initialize a new repository for each thread (or at least for each handler)
    mut message_rabbitmq_repository: MessageRabbitMQRepository,
) -> Result<(), RegisterHandlerExtractContentJobError> {
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
        "📡 Consumer connected to {}, waiting for messages",
        queue.name()
    );
    while let Some(delivery) = consumer.next().await {
        let span = info_span!(
            "Handling queued message",
            queue_name = %queue.name(),
            message_id = %uuid::Uuid::new_v4(),
        );
        let _ = span.enter();

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
                continue;
            }
        };

        let extract_content_job = match ExtractContentJob::try_parsing(&delivery.data) {
            Ok(job) => job,
            Err(error) => {
                error!(
                    ?error,
                    "Failed to parse extract_content_job message data: {}", error
                );
                continue;
            }
        };

        info!("Received extract content job: {:?}\n", extract_content_job);

        match execute_handler(
            s3_repository.clone(),
            extracted_content_meilisearch_repository.clone(),
            &mut message_rabbitmq_repository,
            &extract_content_job,
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
                        ?extract_content_job,
                        "Failed to ack extract_content_job message"
                    );
                }
            }
            Err(error) => {
                error!(
                    ?error,
                    ?extract_content_job,
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
                        ?extract_content_job,
                        "Failed to nack extract_content_job message"
                    );
                }
            }
        }
    }

    Ok(())
}

#[derive(thiserror::Error)]
pub enum ExecuteHandlerExtractContentJobError {
    #[error(transparent)]
    S3RepositoryError(#[from] S3RepositoryError),
    #[error(transparent)]
    ExtractedContentMeilisearchRepositoryError(#[from] ExtractedContentMeilisearchRepositoryError),
    #[error(transparent)]
    MessageRabbitMQRepositoryError(#[from] MessageRabbitMQRepositoryError),
}

impl std::fmt::Debug for ExecuteHandlerExtractContentJobError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        error_chain_fmt(self, f)
    }
}

#[tracing::instrument(
    name = "Executing handler on extract content job",
    skip(
        s3_repository,
        extracted_content_meilisearch_repository,
        message_rabbitmq_repository
    )
)]
pub async fn execute_handler(
    s3_repository: Arc<S3Repository>,
    extracted_content_meilisearch_repository: Arc<ExtractedContentMeilisearchRepository>,
    message_rabbitmq_repository: &mut MessageRabbitMQRepository,
    job: &ExtractContentJob,
) -> Result<(), ExecuteHandlerExtractContentJobError> {
    let ExtractContentJob {
        object_store_path_name,
        ..
    } = job;

    // There is probably a way to stream the content of the file from the S3 bucket,
    // and not put it into memory. Or stream saving the content in a temp file, and
    // access the content with a BufReader.
    let file_content = s3_repository.get_file(object_store_path_name).await?;

    // In-memory file-like object/reader implementing `Seek`.
    // Note: for EPUB (or any format needing a `Seek` impl), we will always need to load the file in-memory ?)
    // TODO: should we wrap it in a `BufReader` ?
    // let file_reader = BufReader::new(file_content.as_slice());
    let file_reader = Cursor::new(file_content);

    let epub_reader =
        EpubReader::from_reader(file_reader, Some(json!({ "file": object_store_path_name })))
            .unwrap();
    let mut xml_reader = xml_reader::build_from_reader(epub_reader);

    let nb_words_per_content = 100;
    let mut generator = extract_content_generator(&mut xml_reader, Some(nb_words_per_content));

    let mut i = 0;
    // Is a limit needed to avoid infinite loop ?
    loop {
        let extracted_content = match generator.as_mut().resume() {
            // .as_mut().resume() {
            GeneratorState::Yielded(content) => content,
            GeneratorState::Complete(_result) => {
                break;
            }
        };

        info!(
            "Extracted content {i}: {}\n{}\n-----\n",
            extracted_content.metadata, extracted_content.content
        );

        // We could decide to continue if we persist the extracted content in more than 1 service.
        extracted_content_meilisearch_repository
            .save(&extracted_content)
            .await?;

        message_rabbitmq_repository
            .publish_content_extracted(&extracted_content)
            .await?;

        i += 1;
    }

    Ok(())
}
