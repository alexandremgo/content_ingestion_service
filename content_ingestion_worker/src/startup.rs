use crate::{
    configuration::{ObjectStorageSettings, RabbitMQSettings, Settings},
    handlers::example,
    repositories::source_file_s3_repository::S3Repository,
};
use lapin::{
    message::DeliveryResult,
    options::{BasicAckOptions, BasicConsumeOptions, QueueDeclareOptions},
    types::FieldTable,
    Channel, Connection,
};
use s3::{creds::Credentials, Bucket, BucketConfiguration, Region};
use secrecy::ExposeSecret;
use tracing::{error, info, info_span, Instrument};

/// Holds the newly built RabbitMQ connection and any server/useful properties
pub struct Application {
    rabbitmq_connection: Connection,
    rabbitmq_queue_name_prefix: String,

    // S3
    // Used for integration tests
    s3_bucket: Bucket,
}

impl Application {
    /// # Parameters
    /// - nb_workers: number of actix-web workers
    ///   if `None`, the number of available physical CPUs is used as the worker count.
    #[tracing::instrument(name = "Building application")]
    pub async fn build(settings: Settings) -> Result<Self, ApplicationBuildError> {
        let s3_bucket = set_up_s3(&settings.object_storage).await?;

        let rabbitmq_connection = get_rabbitmq_connection(&settings.rabbitmq).await?;

        let s3_repository = S3Repository::new(s3_bucket.clone());

        Ok(Self {
            rabbitmq_connection,
            rabbitmq_queue_name_prefix: settings.rabbitmq.queue_name_prefix,
            s3_bucket,
        })
    }

    pub async fn create_rabbitmq_channel(&self) -> Channel {
        self.rabbitmq_connection.create_channel().await.unwrap()
    }

    /// Runs the application until stopped
    ///
    /// This function will block the current thread
    ///
    /// self is moved in order for the application not to drop out of scope
    /// and move into a thread for ex
    pub async fn run_until_stopped(self) -> Result<(), std::io::Error> {
        // Should we use https://docs.rs/lapin/latest/lapin/struct.Connection.html#method.run ?
        self.run().await.unwrap();
        loop {}
    }

    pub async fn run(&self) -> Result<(), std::io::Error> {
        // A channel is a lightweight connection that share a single TCP connection to RabbitMQ
        let channel = self.rabbitmq_connection.create_channel().await.unwrap();

        let queue_name = format!("{}_queue_test", self.rabbitmq_queue_name_prefix);
        info!("🏗️ Declaring queue: {}", queue_name);

        let _queue = channel
            .queue_declare(
                &queue_name,
                QueueDeclareOptions::default(),
                FieldTable::default(),
            )
            .await
            .unwrap();

        let consumer = channel
            .basic_consume(
                &queue_name,
                "tag_foo",
                BasicConsumeOptions::default(),
                FieldTable::default(),
            )
            .await
            .unwrap();

        // TODO: will need to set this in another way
        consumer.set_delegate(move |delivery: DeliveryResult| {
            async move {
                let delivery = match delivery {
                    // Carries the delivery alongside its channel
                    Ok(Some(delivery)) => delivery,
                    // The consumer got canceled
                    Ok(None) => return,
                    // Carries the error and is always followed by Ok(None)
                    Err(error) => {
                        error!(?error, "Failed to consume queue message");
                        return;
                    }
                };

                let my_data = match example::MyData::try_parsing(&delivery.data) {
                    Ok(my_data) => my_data,
                    Err(error) => {
                        error!(?error, "Failed to parse queue message data: {}", error);
                        return;
                    }
                };

                info!(
                    "🦖 Received message properties: {:#?}\n",
                    delivery.properties
                );

                match example::handler(my_data) {
                    Ok(()) => (),
                    Err(error) => {
                        error!(?error, "Failed to handle queue message");
                        return;
                    }
                }

                delivery
                    .ack(BasicAckOptions::default())
                    .await
                    .expect("Failed to ack send_webhook_event message");
            }
            .instrument(info_span!(
                "Handling queued message",
                handler_id = %uuid::Uuid::new_v4()
            ))
        });

        Ok(())
    }
}

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
pub async fn set_up_s3(settings: &ObjectStorageSettings) -> Result<Bucket, ApplicationBuildError> {
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
                    return Err(ApplicationBuildError::S3Error(error));
                }
            }
            _ => return Err(ApplicationBuildError::S3Error(error)),
        }

        info!("Unknown bucket {}, creating it ...", settings.bucket_name);

        Bucket::create_with_path_style(&settings.bucket_name, region, credentials, config).await?;
    }

    info!(
        "Bucket {} has been correctly instantiated",
        settings.bucket_name
    );
    Ok(bucket)
}

#[derive(thiserror::Error, Debug)]
pub enum ApplicationBuildError {
    #[error("S3 credentials error: {0}")]
    S3CredentialsError(#[from] s3::creds::error::CredentialsError),
    #[error(transparent)]
    S3Error(#[from] s3::error::S3Error),
    #[error(transparent)]
    IOError(#[from] std::io::Error),
    #[error(transparent)]
    RabbitMQError(#[from] lapin::Error),
    // #[error(transparent)]
    // MessageRabbitMQRepositoryError(#[from] MessageRabbitMQRepositoryError),
}
