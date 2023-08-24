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
        entities::extract_content_job::ExtractContentJob,
        extractors::extract_content_generator::extract_content_generator,
        readers::{epub_reader::EpubReader, xml_reader},
    },
    helper::error_chain_fmt,
    repositories::source_file_s3_repository::{S3Repository, S3RepositoryError},
};

use common::{
    core::rabbitmq_message_repository::{
        RabbitMQMessageRepository, RabbitMQMessageRepositoryError,
    },
    dtos::extracted_content::ExtractedContentDto,
};

pub const ROUTING_KEY: &str = "extract_content.text.v1";
pub const CONTENT_EXTRACTED_MESSAGE_KEY: &str = "content_extracted.v1";

#[derive(thiserror::Error)]
pub enum RegisterHandlerExtractContentJobError {
    #[error(transparent)]
    RabbitMQError(#[from] lapin::Error),
    #[error(transparent)]
    RabbitMQMessageRepositoryError(#[from] RabbitMQMessageRepositoryError),
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
/// Some repositories (RabbitMQMessageRepository) are initialized inside the handler
/// to avoid sharing some instances (ex: RabbitMQ channel) between each thread
#[tracing::instrument(
    name = "Register message handler",
    skip(
        rabbitmq_consuming_connection,
        s3_repository,
        message_rabbitmq_repository
    )
)]
pub async fn register_handler(
    rabbitmq_consuming_connection: RabbitMQConnection,
    exchange_name: String,
    s3_repository: Arc<S3Repository>,
    // Not an `Arc` shared reference as we want to initialize a new repository for each thread (or at least for each handler)
    message_rabbitmq_repository: RabbitMQMessageRepository,
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

    // One (for publishing) channel for this collection of handlers
    let message_rabbitmq_repository = message_rabbitmq_repository.try_init().await?;
    // TODO: to remove ?
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

            let extract_content_job = match ExtractContentJob::try_parsing(&delivery.data) {
                Ok(job) => job,
                Err(error) => {
                    error!(
                        ?error,
                        "Failed to parse extract_content_job message data: {}", error
                    );
                    return;
                }
            };

            info!("Received extract content job: {:?}\n", extract_content_job);

            match execute_handler(
                s3_repository.clone(),
                &message_rabbitmq_repository,
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
pub enum ExecuteHandlerExtractContentJobError {
    #[error(transparent)]
    S3RepositoryError(#[from] S3RepositoryError),
    #[error(transparent)]
    RabbitMQMessageRepositoryError(#[from] RabbitMQMessageRepositoryError),
    #[error("Error while serializing message data: {0}")]
    JsonError(#[from] serde_json::Error),
}

impl std::fmt::Debug for ExecuteHandlerExtractContentJobError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        error_chain_fmt(self, f)
    }
}

#[tracing::instrument(
    name = "Executing handler on extract content job",
    skip(s3_repository, message_rabbitmq_repository)
)]
pub async fn execute_handler(
    s3_repository: Arc<S3Repository>,
    message_rabbitmq_repository: &RabbitMQMessageRepository,
    job: &ExtractContentJob,
) -> Result<(), ExecuteHandlerExtractContentJobError> {
    let ExtractContentJob {
        object_store_path_name,
        source_type,
        source_initial_name,
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

    let epub_reader = EpubReader::from_reader(
        file_reader,
        Some(json!({ "file": object_store_path_name, "source_initial_name": source_initial_name, "source_type": source_type })),
    )
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

        let json_dto =
            serde_json::to_string(&Into::<ExtractedContentDto>::into(extracted_content))?;
        message_rabbitmq_repository
            .publish(CONTENT_EXTRACTED_MESSAGE_KEY, json_dto.as_bytes())
            .await?;

        i += 1;
    }

    Ok(())
}
