use std::sync::Arc;

use crate::{
    configuration::{ObjectStorageSettings, RabbitMQSettings, Settings},
    handlers::handler_extract_content_job::{self, RegisterHandlerExtractContentJobError},
    repositories::source_file_s3_repository::S3Repository,
};
use common::core::rabbitmq_message_repository::RabbitMQMessageRepository;
use futures::{future::join_all, TryFutureExt};
use lapin::Connection as RabbitMQConnection;
use s3::{creds::Credentials, Bucket, BucketConfiguration, Region};
use secrecy::ExposeSecret;
use tokio::task::JoinHandle;
use tracing::{error, info};

/// Holds the newly built RabbitMQ connection and any server/useful properties
pub struct Application {
    // RabbitMQ
    rabbitmq_publishing_connection: Arc<RabbitMQConnection>,
    rabbitmq_content_exchange_name: String,

    // S3
    // Used for integration tests
    s3_bucket: Bucket,

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

        let message_rabbitmq_repository = RabbitMQMessageRepository::new(
            rabbitmq_publishing_connection.clone(),
            &rabbitmq_content_exchange_name,
        );

        let s3_repository = S3Repository::new(s3_bucket.clone());
        // Sharing the same S3 repository with parallel handlers/threads
        let s3_repository = Arc::new(s3_repository);

        let mut app = Self {
            rabbitmq_publishing_connection,
            rabbitmq_content_exchange_name,
            s3_bucket,
            handlers: vec![],
        };

        app.prepare_message_handlers(
            rabbitmq_consuming_connection,
            message_rabbitmq_repository,
            s3_repository,
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
            s3_repository,
        )
    )]
    pub async fn prepare_message_handlers(
        &mut self,
        rabbitmq_consuming_connection: RabbitMQConnection,
        message_rabbitmq_repository: RabbitMQMessageRepository,
        s3_repository: Arc<S3Repository>,
    ) -> Result<(), ApplicationError> {
        let s3_repository = s3_repository.clone();
        let exchange_name = self.rabbitmq_content_exchange_name.clone();

        // We could have several message handlers running in parallel bound with the same binding key to the same exchange.
        // Or other message handlers bound with a different binding key to the same or another exchange.
        let handler = tokio::spawn(
            handler_extract_content_job::register_handler(
                rabbitmq_consuming_connection,
                exchange_name,
                s3_repository,
                message_rabbitmq_repository.clone(),
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

    pub fn s3_bucket(&self) -> Bucket {
        self.s3_bucket.clone()
    }
}

/// Creates a connection to RabbitMQ
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
