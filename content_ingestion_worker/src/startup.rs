use std::sync::Arc;

use crate::{
    configuration::{MeilisearchSettings, ObjectStorageSettings, RabbitMQSettings, Settings},
    handlers::handler_extract_content_job::{self, RegisterHandlerExtractContentJobError},
    repositories::{
        extracted_content_meilisearch_repository::ExtractedContentMeilisearchRepository,
        message_rabbitmq_repository::MessageRabbitMQRepository,
        source_file_s3_repository::S3Repository,
    },
};
use futures::{future::join_all, Future, TryFutureExt};
use lapin::Connection as RabbitMQConnection;
use meilisearch_sdk::Client as MeilisearchClient;
use s3::{creds::Credentials, Bucket, BucketConfiguration, Region};
use secrecy::ExposeSecret;
use tokio::{join, task::JoinHandle};
use tokio_util::sync::CancellationToken;
use tracing::{error, info};

/// Holds the newly built RabbitMQ connection and any server/useful properties
pub struct Application {
    // RabbitMQ
    rabbitmq_publishing_connection: Arc<RabbitMQConnection>,
    rabbitmq_content_exchange_name: String,

    // S3
    // Used for integration tests
    s3_bucket: Bucket,
    // TODO: like the meilisearch repo: do we need it here ? Or pass it directly to registered handlers ?
    s3_repository: Arc<S3Repository>,

    // Meilisearch
    meilisearch_client: MeilisearchClient,

    // handlers: Vec<Box<dyn Future<Output = Result<(), ApplicationError>>>>,
    handlers: Vec<JoinHandle<Result<(), ApplicationError>>>,
}

impl Application {
    #[tracing::instrument(name = "Building worker application")]
    pub async fn build(settings: Settings) -> Result<Self, ApplicationError> {
        let s3_bucket = set_up_s3(&settings.object_storage).await?;

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

        let s3_repository = S3Repository::new(s3_bucket.clone());
        let s3_repository = Arc::new(s3_repository);

        let meilisearch_client = get_meilisearch_client(&settings.meilisearch);
        let extracted_content_meilisearch_repository = ExtractedContentMeilisearchRepository::new(
            meilisearch_client.clone(),
            settings.meilisearch.extracted_content_index,
        );
        let extracted_content_meilisearch_repository =
            Arc::new(extracted_content_meilisearch_repository);

        let mut app = Self {
            rabbitmq_publishing_connection,
            rabbitmq_content_exchange_name,
            s3_bucket,
            s3_repository,
            meilisearch_client,
            handlers: vec![],
        };

        // Test with delegate
        // app.registers_message_handlers(
        //     rabbitmq_consuming_connection,
        //     message_rabbitmq_repository,
        //     extracted_content_meilisearch_repository,
        // )
        // .await?;

        // Test with 1 thread per binding key/queue (handles message sequentially)
        // TODO: OK PB: the join! should be use in a `run_until_stopped`
        // app.registers_message_handlers_in_threads(
        //     rabbitmq_consuming_connection,
        //     message_rabbitmq_repository,
        //     extracted_content_meilisearch_repository,
        // )
        // .await?;

        app.registers_2_message_handlers_in_threads(
            rabbitmq_consuming_connection,
            message_rabbitmq_repository,
            extracted_content_meilisearch_repository,
        )
        .await?;

        info!("🦄 OK registered !");

        Ok(app)
    }

    /// Runs the application until stopped
    ///
    /// This function will block the current thread
    ///
    /// self is moved in order for the application not to drop out of scope
    /// and move into a thread for ex
    ///
    /// # Parameters
    /// - `cancel_token`: to force the shutdown of the application running as an infinite loop
    pub async fn run_until_stopped(
        self,
        cancel_token: CancellationToken,
    ) -> Result<(), ApplicationError> {
        info!("📡 running until stopped");

        loop {
            if cancel_token.is_cancelled() {
                break;
            }

            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        }

        // Not making it gracefully shutdown
        // self.rabbitmq_connection.close(200, "").await;

        info!("👋 Bye!");
        Ok(())
    }

    // /// Registers queue message handlers with delegate to start the worker
    // #[tracing::instrument(
    //     name = "Preparing to run the worker application with delegate",
    //     skip(
    //         self,
    //         rabbitmq_consuming_connection,
    //         message_rabbitmq_repository,
    //         extracted_content_meilisearch_repository
    //     )
    // )]
    // pub async fn registers_message_handlers(
    //     &self,
    //     rabbitmq_consuming_connection: RabbitMQConnection,
    //     message_rabbitmq_repository: MessageRabbitMQRepository,
    //     extracted_content_meilisearch_repository: Arc<ExtractedContentMeilisearchRepository>,
    // ) -> Result<(), ApplicationError> {
    //     handler_extract_content_job::register_handler(
    //         rabbitmq_consuming_connection,
    //         &self.rabbitmq_content_exchange_name,
    //         self.s3_repository.clone(),
    //         extracted_content_meilisearch_repository.clone(),
    //         message_rabbitmq_repository.clone(),
    //     )
    //     .await?;

    //     Ok(())
    // }

    /// Registers queue message handlers to start the worker
    #[tracing::instrument(
        name = "Preparing to run the worker application",
        skip(
            self,
            rabbitmq_consuming_connection,
            message_rabbitmq_repository,
            extracted_content_meilisearch_repository
        )
    )]
    pub async fn registers_message_handlers_in_threads(
        &self,
        rabbitmq_consuming_connection: RabbitMQConnection,
        message_rabbitmq_repository: MessageRabbitMQRepository,
        extracted_content_meilisearch_repository: Arc<ExtractedContentMeilisearchRepository>,
    ) -> Result<(), ApplicationError> {
        // TODO: OK PB: the join! should be use in a `run_until_stopped`
        let s3_repository = self.s3_repository.clone();
        let exchange_name = self.rabbitmq_content_exchange_name.clone();

        tokio::spawn(handler_extract_content_job::register_handler(
            rabbitmq_consuming_connection,
            exchange_name,
            s3_repository,
            extracted_content_meilisearch_repository.clone(),
            message_rabbitmq_repository.clone(),
        ));

        Ok(())
    }

    /// Registers queue message handlers to start the worker
    #[tracing::instrument(
        name = "Preparing to run the worker application",
        skip(
            self,
            rabbitmq_consuming_connection,
            message_rabbitmq_repository,
            extracted_content_meilisearch_repository
        )
    )]
    pub async fn registers_2_message_handlers_in_threads(
        &mut self,
        rabbitmq_consuming_connection: RabbitMQConnection,
        message_rabbitmq_repository: MessageRabbitMQRepository,
        extracted_content_meilisearch_repository: Arc<ExtractedContentMeilisearchRepository>,
    ) -> Result<(), ApplicationError> {
        // TODO: OK PB: the join! should be use in a `run_until_stopped`
        let s3_repository = self.s3_repository.clone();
        let exchange_name = self.rabbitmq_content_exchange_name.clone();

        let handler = tokio::spawn(
            handler_extract_content_job::register_handler(
                rabbitmq_consuming_connection,
                exchange_name,
                s3_repository,
                extracted_content_meilisearch_repository.clone(),
                message_rabbitmq_repository.clone(),
            )
            .map_err(|e| e.into()),
        );

        self.handlers.push(handler);

        Ok(())
    }

    pub async fn run_2_handlers_until_stopped(self) -> Result<(), ApplicationError> {
        let handler_results = join_all(self.handlers).await;

        info!(
            "Application stopped with the following results: {:?}",
            handler_results
        );

        Ok(())
    }

    /// Registers the messages handlers and runs the application until stopped
    ///
    /// This function will spawn a thread for each handlers and block the current thread
    /// If one handlers fails completely (errors that could not be handled), the all application fails too
    ///
    /// self is moved in order for the application not to drop out of scope
    /// and move into a thread for ex
    ///
    /// # Parameters
    /// - `cancel_token`: to force the shutdown of the application running as an infinite loop
    pub async fn run_handlers_until_stopped(
        self,
        cancel_token: CancellationToken,
        rabbitmq_consuming_connection: RabbitMQConnection,
        message_rabbitmq_repository: MessageRabbitMQRepository,
        extracted_content_meilisearch_repository: Arc<ExtractedContentMeilisearchRepository>,
    ) -> Result<(), ApplicationError> {
        info!("📡 Running handlers until they are stopped");

        // TODO: Fix s3 repository as property of app, not what we want ?
        let s3_repository = self.s3_repository.clone();
        let exchange_name = self.rabbitmq_content_exchange_name.clone();

        let handler_results = join!(handler_extract_content_job::register_handler(
            rabbitmq_consuming_connection,
            exchange_name,
            s3_repository,
            extracted_content_meilisearch_repository.clone(),
            message_rabbitmq_repository.clone(),
        ));

        info!(
            "Application stopped with the following results: {:?}",
            handler_results
        );
        Ok(())
    }

    pub fn s3_bucket(&self) -> Bucket {
        self.s3_bucket.clone()
    }
}

/// Create a connection to RabbitMQ
pub async fn get_rabbitmq_connection(
    config: &RabbitMQSettings,
) -> Result<RabbitMQConnection, lapin::Error> {
    RabbitMQConnection::connect(&config.get_uri(), config.get_connection_properties()).await
}

/// Sets up the S3 object storage
///
/// Each environment will use 1 bucket.
/// This bucket is created if it does not exist yet.
///
/// # Returns
/// An initialized bucket
#[tracing::instrument(name = "Setting up S3 object store")]
pub async fn set_up_s3(settings: &ObjectStorageSettings) -> Result<Bucket, ApplicationError> {
    let region = Region::Custom {
        region: settings.region.to_owned(),
        endpoint: settings.endpoint(),
    };

    let credentials = Credentials::new(
        Some(&settings.username),
        Some(settings.password.expose_secret()),
        None,
        None,
        None,
    )?;

    // Instantiates/gets the bucket if it exists
    let bucket =
        Bucket::new(&settings.bucket_name, region.clone(), credentials.clone())?.with_path_style();

    let config = BucketConfiguration::default();

    // Checks if the bucket exist
    if let Err(error) = bucket.head_object("/").await {
        // Only continues if the error is a bucket not found (404)
        match error {
            s3::error::S3Error::Http(code, _) => {
                if code != 404 {
                    return Err(ApplicationError::S3Error(error));
                }
            }
            _ => return Err(ApplicationError::S3Error(error)),
        }

        info!(
            "🪣 Unknown bucket {}, creating it ...",
            settings.bucket_name
        );

        Bucket::create_with_path_style(&settings.bucket_name, region, credentials, config).await?;
    }

    info!(
        "🪣 Bucket {} has been correctly instantiated",
        settings.bucket_name
    );
    Ok(bucket)
}

/// Set up a client to Meilisearch
pub fn get_meilisearch_client(config: &MeilisearchSettings) -> MeilisearchClient {
    MeilisearchClient::new(config.endpoint(), Some(config.api_key.expose_secret()))
}

#[derive(thiserror::Error, Debug)]
pub enum ApplicationError {
    #[error("S3 credentials error: {0}")]
    S3CredentialsError(#[from] s3::creds::error::CredentialsError),
    #[error(transparent)]
    S3Error(#[from] s3::error::S3Error),
    #[error(transparent)]
    IOError(#[from] std::io::Error),
    #[error(transparent)]
    RabbitMQError(#[from] lapin::Error),
    #[error(transparent)]
    ContentExtractJobError(#[from] RegisterHandlerExtractContentJobError),
}
