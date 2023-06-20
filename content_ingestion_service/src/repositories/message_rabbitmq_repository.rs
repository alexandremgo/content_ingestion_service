use chrono::Utc;
use lapin::{
    options::{BasicPublishOptions, QueueDeclareOptions},
    types::FieldTable,
    BasicProperties, Channel,
};
use tracing::info;

use crate::{domain::entities::content_extract_job::ContentExtractJob, helper::error_chain_fmt};

/// Message broker using RabbitMQ
/// Repository passed to routes handlers so it could be mocked in the future ?
pub struct MessageRabbitMQRepository {
    channel: Channel,
    queue_name_prefix: String,
}

const CONTENT_EXTRACT_JOB_QUEUE: &str = "content_extract_job";

// If we start having several RabbitMQ repository for different domains:
// - `publish` and other internal methods should be moved to a "core" module
// - one repository per domain
impl MessageRabbitMQRepository {
    #[tracing::instrument(name = "Initializing MessageRabbitMQRepository", skip(channel))]
    pub async fn try_new(
        channel: Channel,
        queue_name_prefix: String,
    ) -> Result<Self, MessageRabbitMQRepositoryError> {
        let content_extract_job_queue_name =
            format!("{}_{}", queue_name_prefix, CONTENT_EXTRACT_JOB_QUEUE);

        let queue_declare_options = QueueDeclareOptions::default();
        let _queue = channel
            .queue_declare(
                &content_extract_job_queue_name,
                queue_declare_options,
                FieldTable::default(),
            )
            .await?;

        info!(
            "Successfully declared queue {} with properties: {:?}",
            content_extract_job_queue_name, queue_declare_options
        );

        Ok(Self {
            channel,
            queue_name_prefix,
        })
    }

    /// Internal method to publish a message to a queue
    ///
    /// # Arguments
    /// * `queue_name` - Name of the queue to publish the message to
    /// * `message` - TODO: data ? Message to publish
    #[tracing::instrument(name = "Publishing message", skip(self))]
    async fn publish(
        &self,
        queue_name: &str,
        data: &[u8],
    ) -> Result<(), MessageRabbitMQRepositoryError> {
        let queue_name = format!("{}_{}", self.queue_name_prefix, queue_name);
        let current_time_ms = Utc::now().timestamp_millis() as u64;

        // Publish and only waits for the published confirmation
        // Waiting a 2nd time would wait for a response (ack / nack) from a consumer
        // -> actually no the 2nd await is not waiting for an ack / nack from a consumer
        // TODO: no error if the queue does not exist ...
        let response_first_confirm = self
            .channel
            .basic_publish(
                "",
                &queue_name,
                BasicPublishOptions::default(),
                data,
                BasicProperties::default()
                    .with_timestamp(current_time_ms)
                    .with_message_id(uuid::Uuid::new_v4().to_string().into()),
            )
            .await?;
        info!(
            "🐬 Published message: response_first_confirm: {:?}",
            response_first_confirm
        );

        // TODO: getting a NotRequested - i don't understand what it does 🤷
        let response_second_confirm = response_first_confirm.await?;
        info!(
            "🦖 Published message: response_second_confirm: {:?}",
            response_second_confirm
        );

        Ok(())
    }

    /// Publishes a content extraction job message
    #[tracing::instrument(name = "Publishing content extract job", skip(self))]
    pub async fn publish_content_extract_job(
        &self,
        job: ContentExtractJob,
    ) -> Result<(), MessageRabbitMQRepositoryError> {
        let json_job = serde_json::to_string(&job).unwrap();

        self.publish(CONTENT_EXTRACT_JOB_QUEUE, json_job.as_bytes())
            .await
    }
}

#[derive(thiserror::Error)]
pub enum MessageRabbitMQRepositoryError {
    #[error(transparent)]
    RabbitMQError(#[from] lapin::Error),
}

impl std::fmt::Debug for MessageRabbitMQRepositoryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        error_chain_fmt(self, f)
    }
}
