use std::{io::BufReader, sync::Arc};

use lapin::{
    message::DeliveryResult,
    options::{BasicAckOptions, BasicConsumeOptions, BasicNackOptions, QueueDeclareOptions},
    types::FieldTable,
    Channel,
};
use tracing::{debug, error, info, info_span, Instrument};

use crate::{
    domain::{
        entities::extract_content_job::ExtractContentJob,
        services::extract_content_from_xml::extract_content_from_xml,
    },
    helper::error_chain_fmt,
    repositories::{
        message_rabbitmq_repository::CONTENT_EXTRACT_JOB_QUEUE,
        source_file_s3_repository::{S3Repository, S3RepositoryError},
    },
};

#[derive(thiserror::Error)]
pub enum RegisterHandlerExtractContentJobError {
    #[error(transparent)]
    RabbitMQError(#[from] lapin::Error),
}

impl std::fmt::Debug for RegisterHandlerExtractContentJobError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        error_chain_fmt(self, f)
    }
}

#[tracing::instrument(name = "Register queue handler", skip(channel, s3_repository))]
pub async fn register_handler(
    channel: &Channel,
    queue_name_prefix: &str,
    s3_repository: Arc<S3Repository>,
) -> Result<(), RegisterHandlerExtractContentJobError> {
    let queue_name = format!("{}_{}", queue_name_prefix, CONTENT_EXTRACT_JOB_QUEUE);

    let _queue = channel
        .queue_declare(
            &queue_name,
            QueueDeclareOptions::default(),
            FieldTable::default(),
        )
        .await?;

    let consumer_options = BasicConsumeOptions {
        no_ack: false,
        ..BasicConsumeOptions::default()
    };

    let consumer = channel
        .basic_consume(&queue_name, "", consumer_options, FieldTable::default())
        .await?;

    // let s3_repository = Arc::new(s3_repository);

    // Sets handler on parsed message
    consumer.set_delegate(move |delivery: DeliveryResult| {
        let s3_repository = s3_repository.clone();

        async move {
            let delivery = match delivery {
                // Carries the delivery alongside its channel
                Ok(Some(delivery)) => delivery,
                // The consumer got canceled
                Ok(None) => return,
                // Carries the error and is always followed by Ok(None)
                Err(error) => {
                    error!(?error, "Failed to consume queue message");
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

            match execute_handler(&extract_content_job, s3_repository).await {
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
                        return;
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
                        return;
                    }
                    return;
                }
            }
        }
        .instrument(info_span!(
            "Handling queued message",
            queue_name,
            message_id = %uuid::Uuid::new_v4(),
        ))
    });

    Ok(())
}

#[derive(thiserror::Error)]
pub enum ExecuteHandlerExtractContentJobError {
    #[error(transparent)]
    S3RepositoryError(#[from] S3RepositoryError),
}

impl std::fmt::Debug for ExecuteHandlerExtractContentJobError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        error_chain_fmt(self, f)
    }
}

#[tracing::instrument(name = "Executing handler on extract content job", skip(s3_repository))]
pub async fn execute_handler(
    job: &ExtractContentJob,
    s3_repository: Arc<S3Repository>,
) -> Result<(), ExecuteHandlerExtractContentJobError> {
    let ExtractContentJob {
        object_store_path_name,
        ..
    } = job;

    // There is probably a way to stream the content of the file from the S3 bucket,
    // and not put it into memory. Or stream saving the content in a temp file, and
    // access the content with a BufReader.
    let file_content = s3_repository.get_file(object_store_path_name).await?;
    let buf_reader = BufReader::new(file_content.as_slice());

    debug!(
        "File content of {} from S3: {}",
        object_store_path_name,
        std::str::from_utf8(file_content.as_slice()).unwrap()
    );

    let generator = extract_content_from_xml(buf_reader, Some(100));

    Ok(())
}
