use std::sync::Arc;

use chrono::Utc;
use lapin::{
    options::{BasicPublishOptions, ExchangeDeclareOptions},
    types::FieldTable,
    BasicProperties, Channel, Connection, ExchangeKind,
};
use tracing::info;

use crate::helper::error_chain_fmt;

/// Message broker implemented with RabbitMQ
///
/// If we start having several kind of messages that can be grouped by domains:
/// - `publish` and other internal methods should be moved to a "core" module
/// - one repository per domain
pub struct MessageRabbitMQRepository {
    /// RabbitMQ connection shared with other objects in different threads
    connection: Arc<Connection>,
    /// RabbitMQ channel should not be shared between threads
    /// The channel is wrapped into a "container" that handles its lazy initialization
    /// (so one channel can be created for each thread)
    channel_container: ChannelContainer,
    exchange_name: String,
}

/// Clones only the thread safe part of the repository
///
/// The channel is not cloned because it is not thread safe
impl Clone for MessageRabbitMQRepository {
    /// Only clones the inner RabbitMQ connection and primitive properties, not the RabbitMQ channel.
    fn clone(&self) -> Self {
        Self {
            connection: self.connection.clone(),
            channel_container: ChannelContainer::new(),
            exchange_name: self.exchange_name.clone(),
        }
    }
}

impl MessageRabbitMQRepository {
    /// Builds a RabbitMQ message repository from a RabbitMQ connection
    ///
    /// This constructor does not create a RabbitMQ channel or declare its associated
    /// exchange.
    /// The method `try_init` should be called after, inside each thread using this repository.
    ///
    /// This constructor can be called before spawning threads using this repository
    pub fn new(connection: Arc<Connection>, exchange_name: &str) -> Self {
        Self {
            connection,
            channel_container: ChannelContainer::new(),
            exchange_name: exchange_name.to_string(),
        }
    }

    /// Initializes the repository
    ///
    /// This should be called inside each thread because a RabbitMQ channel should not be shared between threads
    ///
    /// Initializes a RabbitMQ channel and declared the exchange to which this
    /// repository will be associated
    #[tracing::instrument(name = "🏗️ Initializing MessageRabbitMQRepository", skip(self))]
    pub async fn try_init(&mut self) -> Result<(), MessageRabbitMQRepositoryError> {
        let channel = self.channel_container.get_channel(&self.connection).await?;

        // The options could be defined in the configuration in the future,
        // and passed inside the `new` constructor.
        let exchange_declare_options = ExchangeDeclareOptions {
            durable: true,
            ..ExchangeDeclareOptions::default()
        };

        // Idempotent
        channel
            .exchange_declare(
                self.exchange_name.as_str(),
                ExchangeKind::Topic,
                exchange_declare_options,
                FieldTable::default(),
            )
            .await?;

        info!(
            "Successfully declared exchange {} with properties: {:?}",
            self.exchange_name, exchange_declare_options
        );

        Ok(())
    }

    /// Internal method to publish a message to a queue
    ///
    /// # Arguments
    /// * `queue_name` - Name of the queue to publish the message to
    /// * `data` - Data to publish
    #[tracing::instrument(name = "Publishing message", skip(self))]
    async fn publish(
        &mut self,
        message_key: &str,
        data: &[u8],
    ) -> Result<(), MessageRabbitMQRepositoryError> {
        let current_time_ms = Utc::now().timestamp_millis() as u64;

        let channel = self.channel_container.get_channel(&self.connection).await?;

        // Not using publisher confirmation
        channel
            .basic_publish(
                &self.exchange_name,
                message_key,
                BasicPublishOptions::default(),
                data,
                BasicProperties::default()
                    .with_timestamp(current_time_ms)
                    .with_message_id(uuid::Uuid::new_v4().to_string().into()),
            )
            .await?;

        Ok(())
    }
}

#[derive(thiserror::Error)]
pub enum MessageRabbitMQRepositoryError {
    #[error(transparent)]
    RabbitMQError(#[from] lapin::Error),
    #[error("Error while serializing message data: {0}")]
    JsonError(#[from] serde_json::Error),
    #[error("{0}")]
    ChannelInternalError(String),
}

impl std::fmt::Debug for MessageRabbitMQRepositoryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        error_chain_fmt(self, f)
    }
}

/// A kind of singleton, implementing the creation and cleaning logic of a Channel
///
/// The channel is factored into a separate struct so the compiler is able
/// to see that only the `_channel` property is mutated when calling `get_channel`.
struct ChannelContainer {
    _channel: Option<Channel>,
}

impl ChannelContainer {
    pub fn new() -> Self {
        Self { _channel: None }
    }
    /// Handle the "singleton", that can be lazily initialize the channel
    pub async fn get_channel(
        &mut self,
        connection: &Connection,
    ) -> Result<&Channel, MessageRabbitMQRepositoryError> {
        if let Some(ref channel) = self._channel {
            Ok(channel)
        } else {
            let channel = connection.create_channel().await?;
            self._channel = Some(channel);
            return self._channel.as_ref().ok_or(
                MessageRabbitMQRepositoryError::ChannelInternalError(
                    "Channel reference could not be unwrapped".to_string(),
                ),
            );
        }
    }
}