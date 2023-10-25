use std::sync::Arc;

use futures::{future::join_all, TryFutureExt};
use rdkafka::ClientConfig;
use secrecy::ExposeSecret;
use tokio::task::JoinHandle;
use tracing::info;

use crate::{
    configuration::{Settings, MeilisearchSettings},
    controllers::{
        handler_a_grpc_message::{self, GrpcRegisterHandlerError},
        handler_a_kafka_message::{self, KafkaRegisterHandlerError},
    },
    ports::content_repository::{ContentRepository, self},
    repositories::meilisearch_content_repository::MeilisearchContentRepository,
};

// Dependency injection container
shaku::module! {
    pub DIContainer {
        components = [MeilisearchContentRepository],
        providers = []
    }
}

/// Holds all clients and handlers used by our application
pub struct Application {
    // Kafka
    // kafka_client: Arc<RabbitMQConnection>,

    // handlers: Vec<Box<dyn Future<Output = Result<(), ApplicationError>>>>,
    handlers: Vec<JoinHandle<Result<(), ApplicationError>>>,
    // request_handlers or message_handlers ? Can it be both grpc and kafka ?
    // Or consumers ? (but only kafka)
    // Or controllers actually.
}

impl Application {
    #[tracing::instrument(name = "Building application")]
    pub async fn build(settings: Settings) -> Result<Self, ApplicationError> {
        let mut kafka_client_config: ClientConfig = ClientConfig::new();
        kafka_client_config
            .set("group.id", &settings.kafka.group_id_prefix)
            .set(
                "bootstrap.servers",
                format!(
                    "{}:{}",
                    settings.kafka.bootstrap_host, settings.kafka.bootstrap_port
                ),
            )
            .set("enable.partition.eof", "false")
            .set("session.timeout.ms", "6000")
            .set("enable.auto.commit", "false");

        // TODO: message_repository = KafkaMessageRepository ?
        // let message_repository = RabbitMQMessageRepository::new(
        //     rabbitmq_publishing_connection.clone(),
        //     &rabbitmq_content_exchange_name,
        // );

        let meilisearch_client = get_meilisearch_client(&settings.meilisearch);
        let content_repository = MeilisearchContentRepository::new(
            meilisearch_client.clone(),
            settings.meilisearch.contents_index,
        );
        let content_repository = Box::new(content_repository);

        let di_container = DIContainer::builder()
            .with_component_override::<dyn ContentRepository>(content_repository)
            .build();

        let mut app = Self { handlers: vec![] };

        app.prepare_message_handlers(di_container, kafka_client_config).await?;

        Ok(app)
    }

    /// Prepares the asynchronous tasks on which our message handlers will run.
    ///
    /// A "message handler" either consume messages from kafka or handle messages
    /// received from gRPC
    ///
    /// TODO: handlers -> controllers ?
    #[tracing::instrument(
        name = "Preparing the messages handlers",
        skip(self, di_container, kafka_client_config)
    )]
    pub async fn prepare_message_handlers(
        &mut self,
        di_container: DIContainer,
        kafka_client_config: ClientConfig,
        // rabbitmq_consuming_connection: Arc<RabbitMQConnection>,
        // Not an `Arc` shared reference as we want to initialize a new repository for each thread (or at least for each handler)
        // message_repository: RabbitMQMessageRepository,

        // TODO: a message repository for kafka and a message repository for grpc ?
    ) -> Result<(), ApplicationError> {
        // TODO: or just using it directly here and gets the injected repositories etc. ?
        let di_container = Arc::new(di_container);

        let spawn_handler = tokio::spawn(
            handler_a_kafka_message::register_handler(kafka_client_config.clone(), "test_topic")
                .map_err(|e| e.into()),
        );

        self.handlers.push(spawn_handler);

        let spawn_handler =
            tokio::spawn(handler_a_grpc_message::register_handler(di_container.clone()).map_err(|e| e.into()));

        self.handlers.push(spawn_handler);

        // let spawn_handler = tokio::spawn(
        //     handler_a_grpc_request::register_handler(
        //         rabbitmq_consuming_connection.clone(),
        //         exchange_name,
        //         queue_name_prefix,
        //         message_repository.clone(),
        //         content_repository.clone(),
        //     )
        //     .map_err(|e| e.into()),
        // );

        // self.handlers.push(spawn_handler);

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

/// Sets up a client to Meilisearch
pub fn get_meilisearch_client(config: &MeilisearchSettings) -> meilisearch_sdk::Client {
    meilisearch_sdk::Client::new(config.endpoint(), Some(config.api_key.expose_secret()))
}

// TODO: check
#[derive(thiserror::Error, Debug)]
pub enum ApplicationError {
    #[error(transparent)]
    IOError(#[from] std::io::Error),
    #[error(transparent)]
    KafkaRegisterHandlerError(#[from] KafkaRegisterHandlerError),
    #[error(transparent)]
    GrpcRegisterHandlerError(#[from] GrpcRegisterHandlerError),
}
