use std::sync::Arc;

use crate::{
    configuration::{MeilisearchSettings, RabbitMQSettings, Settings},
    handlers::handler_content_extracted::{self, RegisterHandlerContentExtractedError},
    repositories::meilisearch_content_repository::MeilisearchContentRepository,
};
use common::core::rabbitmq_message_repository::RabbitMQMessageRepository;
use futures::{future::join_all, TryFutureExt};
use lapin::Connection as RabbitMQConnection;
use meilisearch_sdk::Client as MeilisearchClient;
use secrecy::ExposeSecret;
use tokio::task::JoinHandle;
use tracing::{error, info};

/// Holds the newly built RabbitMQ connection and any server/useful properties
pub struct Application {
    // RabbitMQ
    rabbitmq_publishing_connection: Arc<RabbitMQConnection>,
    rabbitmq_content_exchange_name: String,
    rabbitmq_queue_name_prefix: String,

    // Meilisearch
    meilisearch_client: MeilisearchClient,

    // handlers: Vec<Box<dyn Future<Output = Result<(), ApplicationError>>>>,
    handlers: Vec<JoinHandle<Result<(), ApplicationError>>>,
}

impl Application {
    #[tracing::instrument(name = "Building application")]
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

        let message_repository = RabbitMQMessageRepository::new(
            rabbitmq_publishing_connection.clone(),
            &rabbitmq_content_exchange_name,
        );

        let meilisearch_client = get_meilisearch_client(&settings.meilisearch);
        let content_repository = MeilisearchContentRepository::new(
            meilisearch_client.clone(),
            settings.meilisearch.contents_index,
        );
        // Sharing the same meilisearch repository with parallel handlers/threads
        let content_repository = Arc::new(content_repository);

        let mut app = Self {
            rabbitmq_publishing_connection,
            rabbitmq_content_exchange_name,
            rabbitmq_queue_name_prefix: settings.rabbitmq.queue_name_prefix,
            meilisearch_client,
            handlers: vec![],
        };

        app.prepare_message_handlers(
            rabbitmq_consuming_connection,
            message_repository,
            content_repository,
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
            message_repository,
            content_repository
        )
    )]
    pub async fn prepare_message_handlers(
        &mut self,
        rabbitmq_consuming_connection: RabbitMQConnection,
        // Not an `Arc` shared reference as we want to initialize a new repository for each thread (or at least for each handler)
        message_repository: RabbitMQMessageRepository,
        content_repository: Arc<MeilisearchContentRepository>,
    ) -> Result<(), ApplicationError> {
        let exchange_name = self.rabbitmq_content_exchange_name.clone();
        let queue_name_prefix = self.rabbitmq_queue_name_prefix.clone();

        // We could have several message handlers running in parallel bound with the same binding key to the same exchange.
        // Or other message handlers bound with a different binding key to the same or another exchange.
        let handler = tokio::spawn(
            handler_content_extracted::register_handler(
                rabbitmq_consuming_connection,
                exchange_name,
                queue_name_prefix,
                message_repository.clone(),
                content_repository.clone(),
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

/// Create a connection to RabbitMQ
pub async fn get_rabbitmq_connection(
    config: &RabbitMQSettings,
) -> Result<RabbitMQConnection, lapin::Error> {
    RabbitMQConnection::connect(&config.get_uri(), config.get_connection_properties()).await
}

/// Set up a client to Meilisearch
pub fn get_meilisearch_client(config: &MeilisearchSettings) -> MeilisearchClient {
    MeilisearchClient::new(config.endpoint(), Some(config.api_key.expose_secret()))
}

#[derive(thiserror::Error, Debug)]
pub enum ApplicationError {
    #[error(transparent)]
    IOError(#[from] std::io::Error),
    #[error(transparent)]
    RabbitMQError(#[from] lapin::Error),
    #[error(transparent)]
    ContentExtractedHandlerError(#[from] RegisterHandlerContentExtractedError),
}
