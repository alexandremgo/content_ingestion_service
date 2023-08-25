use chrono::Utc;
use futures::StreamExt;
use lapin::{
    options::{BasicConsumeOptions, BasicPublishOptions, ExchangeDeclareOptions},
    types::FieldTable,
    BasicProperties, Channel, Connection, ExchangeKind,
};
use std::sync::Arc;
use tracing::{error, info, info_span, Instrument};
use uuid::Uuid;

use crate::helper::error_chain_fmt;

/// Message repository implemented with RabbitMQ
///
/// To publish messages from a service to a given exchange
///
/// The enum definition gatekeeps functionalities if the repository is not ready (not initialized).
pub enum RabbitMQMessageRepository {
    Ready {
        /// RabbitMQ connection shared with other objects in different threads
        connection: Arc<Connection>,
        /// RabbitMQ channel should not be shared between threads, and is created for each thread
        /// (so one channel can be created for each thread)
        channel: Channel,
        exchange_name: String,
    },
    Idle {
        /// RabbitMQ connection shared with other objects in different threads
        connection: Arc<Connection>,
        exchange_name: String,
    },
}

pub const CONTENT_EXTRACTED_MESSAGE_KEY: &str = "content_extracted.v1";

/// Clones only the thread safe part of the repository
///
/// The channel is not cloned because it is not thread safe
impl Clone for RabbitMQMessageRepository {
    /// Only clones the inner RabbitMQ connection and primitive properties, not the RabbitMQ channel.
    /// The cloned Repository is in the idle state, waiting for an initialization.
    fn clone(&self) -> Self {
        match self {
            Self::Idle {
                connection,
                exchange_name,
                ..
            }
            | Self::Ready {
                connection,
                exchange_name,
                ..
            } => Self::Idle {
                connection: connection.clone(),
                exchange_name: exchange_name.clone(),
            },
        }
    }
}

impl RabbitMQMessageRepository {
    /// Builds a RabbitMQ message repository from a RabbitMQ connection
    ///
    /// This constructor does not create a RabbitMQ channel or declare its associated
    /// exchange.
    /// The method `try_init` should be called after, inside each thread using this repository.
    ///
    /// This constructor can be called before spawning threads using this repository
    pub fn new(connection: Arc<Connection>, exchange_name: &str) -> Self {
        Self::Idle {
            connection,
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
    pub async fn try_init(self) -> Result<Self, RabbitMQMessageRepositoryError> {
        if let Self::Ready { .. } = self {
            info!("Already initialized",);
            return Ok(self);
        }

        match self {
            Self::Ready { .. } => {
                info!("Already initialized",);
                return Ok(self);
            }

            Self::Idle {
                connection,
                exchange_name,
            } => {
                let channel = connection.create_channel().await?;

                // The options could be defined in the configuration in the future,
                // and passed inside the `new` constructor.
                let exchange_declare_options = ExchangeDeclareOptions {
                    durable: true,
                    ..ExchangeDeclareOptions::default()
                };

                // Idempotent
                channel
                    .exchange_declare(
                        exchange_name.as_str(),
                        ExchangeKind::Topic,
                        exchange_declare_options,
                        FieldTable::default(),
                    )
                    .await?;

                info!(
                    "Successfully declared exchange {} with properties: {:?}",
                    exchange_name, exchange_declare_options
                );

                Ok(Self::Ready {
                    connection,
                    channel,
                    exchange_name,
                })
            }
        }
    }

    /// Publishes a message with a given routing key
    ///
    /// # Arguments
    /// * `routing_key` - routing key to publish the message to
    /// * `data` - Data to publish
    #[tracing::instrument(name = "Publishing message", skip(self))]
    pub async fn publish(
        &self,
        routing_key: &str,
        data: &[u8],
    ) -> Result<(), RabbitMQMessageRepositoryError> {
        match self {
            Self::Idle { .. } => {
                return Err(RabbitMQMessageRepositoryError::NotInitialized(
                    "Cannot publish message, repository is not initialized".to_string(),
                ))
            }

            Self::Ready {
                channel,
                exchange_name,
                ..
            } => {
                let current_time_ms = Utc::now().timestamp_millis() as u64;

                // Not using publisher confirmation
                channel
                    .basic_publish(
                        exchange_name,
                        routing_key,
                        BasicPublishOptions::default(),
                        data,
                        BasicProperties::default()
                            .with_timestamp(current_time_ms)
                            .with_message_id(Uuid::new_v4().to_string().into()),
                    )
                    .await?;

                Ok(())
            }
        }
    }

    /// RPC call: publishes a message with a given routing key and waits for a response
    ///
    /// Note: Reply messages sent using this RPC mechanism are in general not fault-tolerant:
    /// they will be discarded if the client that published the original request subsequently disconnects.
    /// The assumption is that an RPC client will reconnect and submit another request in this case.
    ///
    /// # Arguments
    /// * `routing_key` - routing key to publish the message to
    /// * `data` - Data to publish
    #[tracing::instrument(name = "Publishing message", skip(self))]
    pub async fn rpc_call(
        &self,
        routing_key: &str,
        data: &[u8],
    ) -> Result<(), RabbitMQMessageRepositoryError> {
        match self {
            Self::Idle { .. } => {
                return Err(RabbitMQMessageRepositoryError::NotInitialized(
                    "Cannot RPC call, repository is not initialized".to_string(),
                ))
            }

            Self::Ready {
                channel,
                exchange_name,
                ..
            } => {
                let current_time_ms = Utc::now().timestamp_millis() as u64;

                channel
                    .basic_publish(
                        exchange_name,
                        routing_key,
                        BasicPublishOptions::default(),
                        data,
                        BasicProperties::default()
                            .with_reply_to("amq.rabbitmq.reply-to".into())
                            .with_timestamp(current_time_ms)
                            .with_message_id(Uuid::new_v4().to_string().into()),
                    )
                    .await?;

                // Waits for an answer on the pseudo-queue `amq.rabbitmq.reply-to` and the default RabbitMQ exchange
                let mut consumer = channel
                    .basic_consume(
                        "amq.rabbitmq.reply-to",
                        "",
                        BasicConsumeOptions {
                            no_ack: true,
                            ..BasicConsumeOptions::default()
                        },
                        FieldTable::default(),
                    )
                    .await?;

                while let Some(delivery) = consumer.next().await {
                    async {
                        let delivery = match delivery {
                            // Carries the delivery alongside its channel
                            Ok(delivery) => delivery,
                            // Carries the error and is always followed by Ok(None)
                            Err(error) => {
                                error!(
                                    ?error,
                                    "Failed to consume response message on queue amq.rabbitmq.reply-to",
                                );
                                return;
                            }
                        };

                        info!("🦄 Received response: {:?}\n", delivery);
                    }
                    .instrument(info_span!(
                        "Handling RPC response",
                        message_id = %uuid::Uuid::new_v4(),
                    ))
                    .await
                }

                Ok(())
            }
        }
    }
}

#[derive(thiserror::Error)]
pub enum RabbitMQMessageRepositoryError {
    #[error(transparent)]
    RabbitMQError(#[from] lapin::Error),
    #[error("{0}")]
    ChannelInternalError(String),
    #[error("{0}")]
    NotInitialized(String),
}

impl std::fmt::Debug for RabbitMQMessageRepositoryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        error_chain_fmt(self, f)
    }
}
