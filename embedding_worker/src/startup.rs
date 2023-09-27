use crate::{
    configuration::{QdrantSettings, RabbitMQSettings, Settings},
    domain::services::huggingface_embedding::{
        HuggingFaceEmbeddingsService, HuggingFaceEmbeddingsServiceError,
    },
    handlers::handler_content_extracted::{self, RegisterHandlerContentExtractedError},
    repositories::content_point_qdrant_repository::{
        ContentPointQdrantRepository, ContentPointQdrantRepositoryError,
    },
};
use common::core::rabbitmq_message_repository::RabbitMQMessageRepository;
use futures::{future::join_all, TryFutureExt};
use lapin::Connection as RabbitMQConnection;
use qdrant_client::prelude::{QdrantClient, QdrantClientConfig};
use std::sync::Arc;
use tokio::task::JoinHandle;
use tracing::{error, info};

/// Holds the newly built RabbitMQ connection and any server/useful properties
pub struct Application {
    // RabbitMQ
    _rabbitmq_publishing_connection: Arc<RabbitMQConnection>,
    rabbitmq_content_exchange_name: String,
    rabbitmq_queue_name_prefix: String,

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

        let message_repository = RabbitMQMessageRepository::new(
            rabbitmq_publishing_connection.clone(),
            &rabbitmq_content_exchange_name,
        );

        // TODO: Qdrant client is using grpc channel (?): should we have 1 channel per thread ?
        // And do the same initialization than with RabbitMQ ?
        // If use Qdrant during integration test: create several qdrant client
        let qdrant_client = get_qdrant_client(&settings.qdrant)?;
        let content_point_qdrant_repository = ContentPointQdrantRepository::try_new(
            qdrant_client,
            &settings.qdrant.collection,
            &settings.qdrant.collection_distance,
            settings.qdrant.collection_vector_size,
        )
        .await?;
        // Sharing the same qdrant repository with parallel handlers/threads
        let content_point_qdrant_repository = Arc::new(content_point_qdrant_repository);

        // The model type could come from the configuration
        let embeddings_service = HuggingFaceEmbeddingsService::new();

        let mut app = Self {
            _rabbitmq_publishing_connection: rabbitmq_publishing_connection,
            rabbitmq_content_exchange_name,
            rabbitmq_queue_name_prefix: settings.rabbitmq.queue_name_prefix,
            handlers: vec![],
        };

        app.prepare_message_handlers(
            rabbitmq_consuming_connection,
            message_repository,
            content_point_qdrant_repository,
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
            message_repository,
            content_point_qdrant_repository,
            embeddings_service
        )
    )]
    pub async fn prepare_message_handlers(
        &mut self,
        rabbitmq_consuming_connection: RabbitMQConnection,
        // Not an `Arc` shared reference as we want to initialize a new repository for each thread (or at least for each handler)
        message_repository: RabbitMQMessageRepository,
        content_point_qdrant_repository: Arc<ContentPointQdrantRepository>,
        embeddings_service: HuggingFaceEmbeddingsService,
    ) -> Result<(), ApplicationError> {
        let exchange_name = self.rabbitmq_content_exchange_name.clone();
        let queue_name_prefix = self.rabbitmq_queue_name_prefix.clone();
        let embeddings_service = Arc::new(embeddings_service);

        // We could have several message handlers running in parallel bound with the same binding key to the same exchange.
        // Or other message handlers bound with a different binding key to the same or another exchange.
        let handler = tokio::spawn(
            handler_content_extracted::register_handler(
                rabbitmq_consuming_connection,
                exchange_name,
                queue_name_prefix,
                message_repository.clone(),
                content_point_qdrant_repository.clone(),
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

/// Set up a client to Qdrant
pub fn get_qdrant_client(config: &QdrantSettings) -> Result<QdrantClient, ApplicationError> {
    let qdrant_config = QdrantClientConfig::from_url(&config.get_grpc_base_url());
    QdrantClient::new(Some(qdrant_config)).map_err(|e| ApplicationError::QdrantError(e.to_string()))
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
    #[error("Error from Qdrant: {0}")]
    QdrantError(String),
    #[error(transparent)]
    ContentPointQdrantRepositoryError(#[from] ContentPointQdrantRepositoryError),
}
