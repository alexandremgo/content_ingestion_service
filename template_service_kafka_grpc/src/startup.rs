use futures::{future::join_all, TryFutureExt};
use rdkafka::ClientConfig;
use tokio::task::JoinHandle;
use tracing::info;

use crate::{
    configuration::Settings,
    controllers::{handler_a_kafka_message::{self, KafkaRegisterHandlerError}, handler_a_grpc_message::{self, GrpcRegisterHandlerError}},
};

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


        let mut app = Self { handlers: vec![] };

        app.prepare_message_handlers(kafka_client_config).await?;

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
        skip(self, kafka_client_config)
    )]
    pub async fn prepare_message_handlers(
        &mut self,
        kafka_client_config: ClientConfig,
        // rabbitmq_consuming_connection: Arc<RabbitMQConnection>,
        // Not an `Arc` shared reference as we want to initialize a new repository for each thread (or at least for each handler)
        // message_repository: RabbitMQMessageRepository,

        // TODO: a message repository for kafka and a message repository for grpc ?
    ) -> Result<(), ApplicationError> {
        let spawn_handler = tokio::spawn(
            handler_a_kafka_message::register_handler(kafka_client_config.clone(), "test_topic")
                .map_err(|e| e.into()),
        );

        self.handlers.push(spawn_handler);


        let spawn_handler = tokio::spawn(
            handler_a_grpc_message::register_handler()
                .map_err(|e| e.into()),
        );

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
