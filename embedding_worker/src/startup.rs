use std::sync::Arc;

use crate::{
    configuration::{RabbitMQSettings, Settings},
    domain::services::huggingface_embedding::{
        HuggingFaceEmbeddingsService, HuggingFaceEmbeddingsServiceError,
    },
    handlers::handler_content_extracted::{self, RegisterHandlerContentExtractedError},
    repositories::message_rabbitmq_repository::MessageRabbitMQRepository,
};
use futures::{future::join_all, TryFutureExt};
use lapin::Connection as RabbitMQConnection;
use rust_bert::pipelines::sentence_embeddings::SentenceEmbeddingsModelType;
use tokio::task::JoinHandle;
use tracing::{error, info};

/// Holds the newly built RabbitMQ connection and any server/useful properties
pub struct Application {
    // RabbitMQ
    rabbitmq_publishing_connection: Arc<RabbitMQConnection>,
    rabbitmq_content_exchange_name: String,

    // handlers: Vec<Box<dyn Future<Output = Result<(), ApplicationError>>>>,
    handlers: Vec<JoinHandle<Result<(), ApplicationError>>>,
}

impl Application {
    #[tracing::instrument(name = "Building worker application")]
    pub async fn build(settings: Settings) -> Result<Self, ApplicationError> {
        // TODO: handle connections with a re-connection strategy
        // One connection for consuming messages, one for publishing messages
        let rabbitmq_consuming_connection = get_rabbitmq_connection(&settings.rabbitmq).await?;
        let rabbitmq_publishing_connection =
            Arc::new(get_rabbitmq_connection(&settings.rabbitmq).await?);

        let rabbitmq_content_exchange_name = format!(
            "{}_{}",
            settings.rabbitmq.exchange_name_prefix, settings.rabbitmq.content_exchange
        );

        let message_rabbitmq_repository = MessageRabbitMQRepository::new(
            rabbitmq_publishing_connection.clone(),
            &rabbitmq_content_exchange_name,
        );

        // The model type could come from the configuration
        let embeddings_service = HuggingFaceEmbeddingsService::new();

        let mut app = Self {
            rabbitmq_publishing_connection,
            rabbitmq_content_exchange_name,
            handlers: vec![],
        };

        app.prepare_message_handlers(
            rabbitmq_consuming_connection,
            message_rabbitmq_repository,
            embeddings_service,
        )
        .await?;

        Ok(app)
    }

    /// Prepares the asynchronous tasks on which our message handlers will run.
    ///
    /// A "message handler" consumes messages from a (generated) queue bound to with a specific binding key to the given exchange
    #[tracing::instrument(
        name = "Preparing the messages handlers",
        skip(
            self,
            rabbitmq_consuming_connection,
            message_rabbitmq_repository,
            embeddings_service
        )
    )]
    pub async fn prepare_message_handlers(
        &mut self,
        rabbitmq_consuming_connection: RabbitMQConnection,
        message_rabbitmq_repository: MessageRabbitMQRepository,
        embeddings_service: HuggingFaceEmbeddingsService,
    ) -> Result<(), ApplicationError> {
        let exchange_name = self.rabbitmq_content_exchange_name.clone();
        let embeddings_service = Arc::new(embeddings_service);

        // We could have several message handlers running in parallel bound with the same binding key to the same exchange.
        // Or other message handlers bound with a different binding key to the same or another exchange.
        let handler = tokio::spawn(
            handler_content_extracted::register_handler(
                rabbitmq_consuming_connection,
                exchange_name,
                message_rabbitmq_repository.clone(),
                embeddings_service.clone(),
            )
            .map_err(|e| e.into()),
        );

        self.handlers.push(handler);

        Ok(())
    }

    /// Runs the application until stopped
    ///
    /// self is moved in order for the application not to drop out of scope
    /// and move into a thread for ex
    pub async fn run_until_stopped(self) -> Result<(), ApplicationError> {
        let handler_results = join_all(self.handlers).await;

        info!(
            "Application stopped with the following results: {:?}",
            handler_results
        );

        info!("👋 Bye!");
        Ok(())
    }
}

/// Creates a connection to RabbitMQ
pub async fn get_rabbitmq_connection(
    config: &RabbitMQSettings,
) -> Result<RabbitMQConnection, lapin::Error> {
    RabbitMQConnection::connect(&config.get_uri(), config.get_connection_properties()).await
}

#[derive(thiserror::Error, Debug)]
pub enum ApplicationError {
    #[error(transparent)]
    IOError(#[from] std::io::Error),
    #[error(transparent)]
    RabbitMQError(#[from] lapin::Error),
    #[error(transparent)]
    RegisterHandlerContentExtractedError(#[from] RegisterHandlerContentExtractedError),
    #[error(transparent)]
    HuggingFaceEmbeddingsServiceError(#[from] HuggingFaceEmbeddingsServiceError),
}