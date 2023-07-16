use std::sync::Arc;

use crate::{
    configuration::{MeilisearchSettings, ObjectStorageSettings, RabbitMQSettings, Settings},
    handlers::handler_extract_content_job::{self, RegisterHandlerExtractContentJobError},
    repositories::{
        extracted_content_meilisearch_repository::ExtractedContentMeilisearchRepository,
        source_file_s3_repository::S3Repository,
    },
};
use lapin::{Channel, Connection};
use meilisearch_sdk::Client as MeilisearchClient;
use s3::{creds::Credentials, Bucket, BucketConfiguration, Region};
use secrecy::ExposeSecret;
use tokio_util::sync::CancellationToken;
use tracing::{error, info};

/// Holds the newly built RabbitMQ connection and any server/useful properties
pub struct Application {
    // RabbitMQ
    rabbitmq_connection: Connection,
    rabbitmq_queue_name_prefix: String,

    // S3
    // Used for integration tests
    s3_bucket: Bucket,
    // TODO: like the meilisearch repo: do we need it here ? Or pass it directly to registered handlers ?
    s3_repository: Arc<S3Repository>,

    // Meilisearch
    meilisearch_client: MeilisearchClient,
}

impl Application {
    #[tracing::instrument(name = "Building worker application")]
    pub async fn build(settings: Settings) -> Result<Self, ApplicationError> {
        let s3_bucket = set_up_s3(&settings.object_storage).await?;

        let rabbitmq_connection = get_rabbitmq_connection(&settings.rabbitmq).await?;

        let s3_repository = S3Repository::new(s3_bucket.clone());
        let s3_repository = Arc::new(s3_repository);

        let meilisearch_client = get_meilisearch_client(&settings.meilisearch);
        let extracted_content_meilisearch_repository = ExtractedContentMeilisearchRepository::new(
            meilisearch_client.clone(),
            settings.meilisearch.extracted_content_index,
        );
        let extracted_content_meilisearch_repository =
            Arc::new(extracted_content_meilisearch_repository);

        let app = Self {
            rabbitmq_connection,
            rabbitmq_queue_name_prefix: settings.rabbitmq.queue_name_prefix,
            s3_bucket,
            s3_repository,
            meilisearch_client,
        };

        app.registers_message_handlers(extracted_content_meilisearch_repository)
            .await?;

        Ok(app)
    }

    /// A channel is a lightweight connection that share a single TCP connection to RabbitMQ
    pub async fn create_rabbitmq_channel(&self) -> Result<Channel, lapin::Error> {
        self.rabbitmq_connection.create_channel().await
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

    /// Registers queue message handlers to start the worker
    #[tracing::instrument(
        name = "Preparing to run the worker application",
        skip(self, extracted_content_meilisearch_repository)
    )]
    pub async fn registers_message_handlers(
        &self,
        extracted_content_meilisearch_repository: Arc<ExtractedContentMeilisearchRepository>,
    ) -> Result<(), ApplicationError> {
        let channel = self.create_rabbitmq_channel().await?;

        handler_extract_content_job::register_handler(
            &channel,
            &self.rabbitmq_queue_name_prefix,
            self.s3_repository.clone(),
            extracted_content_meilisearch_repository.clone(),
        )
        .await?;

        Ok(())
    }

    pub fn s3_bucket(&self) -> Bucket {
        self.s3_bucket.clone()
    }
}

/// Create a connection to RabbitMQ
pub async fn get_rabbitmq_connection(
    config: &RabbitMQSettings,
) -> Result<lapin::Connection, lapin::Error> {
    lapin::Connection::connect(&config.get_uri(), config.get_connection_properties()).await
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
