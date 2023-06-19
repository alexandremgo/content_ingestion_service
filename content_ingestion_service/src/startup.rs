use actix_web::{
    dev::Server,
    web::{self, Data},
    App, HttpServer,
};
use s3::{creds::Credentials, Bucket, BucketConfiguration, Region};
use secrecy::ExposeSecret;
use sqlx::{postgres::PgPoolOptions, PgPool};
use std::net::TcpListener;
use tracing::info;
use tracing_actix_web::TracingLogger;

use crate::{
    configuration::{DatabaseSettings, ObjectStorageSettings, RabbitMQSettings, Settings},
    repositories::{
        message_rabbitmq_repository::{MessageRabbitMQRepository, MessageRabbitMQRepositoryError},
        source_file_s3_repository::S3Repository,
        source_meta_postgres_repository::SourceMetaPostgresRepository,
    },
    routes::{add_source_files::add_source_files, health_check},
};

/// Holds the newly built server, and some useful properties
pub struct Application {
    // Server
    server: Server,
    port: u16,

    // S3
    // Used for integration tests
    s3_bucket: Bucket,

    // RabbitMQ
    // TODO: are they needed for integrations tests ? If not to remove
    // rabbitmq_connection: lapin::Connection,
    rabbitmq_queue_name_prefix: String,
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
    #[error(transparent)]
    MessageRabbitMQRepositoryError(#[from] MessageRabbitMQRepositoryError),
}

impl Application {
    #[tracing::instrument(name = "Building application")]
    pub async fn build(settings: Settings) -> Result<Self, ApplicationBuildError> {
        let connection_pool = get_connection_pool(&settings.database);

        let address = format!(
            "{}:{}",
            settings.application.host, settings.application.port
        );
        let listener = TcpListener::bind(address)?;
        let port = listener.local_addr().unwrap().port();

        let s3_bucket = set_up_s3(&settings.object_storage).await?;

        let rabbitmq_connection = get_rabbitmq_connection(&settings.rabbitmq).await?;
        // let rabbitmq_channel = create_rabbitmq_channel(&rabbitmq_connection).await?;

        let s3_repository = S3Repository::new(s3_bucket.clone());
        let source_meta_repository = SourceMetaPostgresRepository::new();
        // FIXME:
        // let message_rabbitmq_repository = MessageRabbitMQRepository::try_new(
        //     rabbitmq_channel,
        //     settings.rabbitmq.queue_name_prefix.clone(),
        // )
        // .await?;

        let server = run(
            listener,
            connection_pool,
            rabbitmq_connection,
            s3_repository,
            source_meta_repository,
            // message_rabbitmq_repository,
        )?;

        Ok(Self {
            server,
            port,
            s3_bucket,
            // rabbitmq_connection,
            rabbitmq_queue_name_prefix: settings.rabbitmq.queue_name_prefix,
        })
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    pub fn s3_bucket(&self) -> Bucket {
        self.s3_bucket.clone()
    }

    /// This function only returns when the application is stopped
    pub async fn run_until_stopped(self) -> Result<(), std::io::Error> {
        self.server.await
    }
}

async fn init_rabbitmq_repository(
    rabbitmq_connection: Data<lapin::Connection>,
) -> Result<MessageRabbitMQRepository, String> {
    let rabbitmq_channel = create_rabbitmq_channel(&rabbitmq_connection).await.unwrap();

    let message_rabbitmq_repository =
        MessageRabbitMQRepository::try_new(rabbitmq_channel, "this_is_joke".to_string())
            .await
            .unwrap();

    Ok(message_rabbitmq_repository)
}

/// listener: the consumer binds their own port
///
/// TracingLogger middleware: helps collecting telemetry data.
/// It generates a unique identifier for each incoming request: `request_id`.
pub fn run(
    listener: TcpListener,
    db_pool: PgPool,
    rabbitmq_connection: lapin::Connection,
    s3_repository: S3Repository,
    source_meta_repository: SourceMetaPostgresRepository,
    // message_rabbitmq_repository: MessageRabbitMQRepository,
) -> Result<Server, std::io::Error> {
    // Wraps the connection to a db in smart pointers
    let db_pool = Data::new(db_pool);

    // Wraps repositories to register them and access them from handlers
    let s3_repository = Data::new(s3_repository);
    let source_meta_repository = Data::new(source_meta_repository);
    // FIXME: should we create a channel per thread ?
    // let message_rabbitmq_repository = Data::new(message_rabbitmq_repository);
    // Sharing the RabbitMQ connection between each thread
    // But each thread will use their own channel
    let rabbitmq_connection = Data::new(rabbitmq_connection);

    let message_rabbitmq_repository_factory = move || {
        let rabbitmq_connection = rabbitmq_connection.clone();

        async {
            info!("🦀🦀🦀 data factory called !");
            // Ok("ok data factory test test".to_string());
            init_rabbitmq_repository(rabbitmq_connection).await
        }
    };

    // `move` to capture `connection` from the surrounding environment
    let server = HttpServer::new(move || {
        info!("🦖 actix-web worker created");

        App::new()
            .wrap(TracingLogger::default())
            .route("/health_check", web::get().to(health_check))
            // FIXME: This way of registering is not needed anymore ?
            // .configure(|cfg| register_add_source_files(cfg, &rabbitmq_channel))
            .route("/add_source_files", web::post().to(add_source_files))
            // .route("/ingest_document", web::post().to(publish_newsletter))
            // Registers the db connection as part of the application state
            // Gets a pointer copy and attach it to the application state
            .app_data(db_pool.clone())
            .app_data(s3_repository.clone())
            .app_data(source_meta_repository.clone())
            // .app_data(rabbitmq_connection.clone())
            // .app_data(message_rabbitmq_repository.clone())
            .data_factory(message_rabbitmq_repository_factory.clone())
    })
    .workers(5)
    .listen(listener)?
    .run();

    // No await
    Ok(server)
}

pub fn get_connection_pool(settings: &DatabaseSettings) -> PgPool {
    PgPoolOptions::new()
        .acquire_timeout(std::time::Duration::from_secs(2))
        .connect_lazy_with(settings.with_db())
}

/// Sets up the S3 object storage
///
/// Each environment will use 1 bucket.
/// This bucket is created if it does not exist yet.
///
/// TODO: doing the same for Postgres and not rely on bash script for migration ?
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

async fn get_rabbitmq_connection(
    config: &RabbitMQSettings,
) -> Result<lapin::Connection, lapin::Error> {
    lapin::Connection::connect(&config.get_uri(), config.get_connection_properties()).await
}

// Not a method/self because we need a channel to run the server, before building the application
pub async fn create_rabbitmq_channel(
    connection: &lapin::Connection,
) -> Result<lapin::Channel, lapin::Error> {
    connection.create_channel().await
}
