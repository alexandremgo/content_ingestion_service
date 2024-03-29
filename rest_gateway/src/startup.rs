use actix_web::{
    dev::Server,
    web::{self, Data},
    App, HttpServer,
};
use common::core::rabbitmq_message_repository::{
    RabbitMQMessageRepository, RabbitMQMessageRepositoryError,
};
use s3::{creds::Credentials, Bucket, BucketConfiguration, Region};
use secrecy::ExposeSecret;
use sqlx::{postgres::PgPoolOptions, PgPool};
use std::{net::TcpListener, sync::Arc};
use tracing::info;
use tracing_actix_web::TracingLogger;

use crate::{
    configuration::{DatabaseSettings, ObjectStorageSettings, RabbitMQSettings, Settings},
    controllers::{add_source_files, create_account, health_check, log_in_account, search_content},
    middlewares::jwt_authentication::middleware::RequireAuth,
    repositories::{
        jwt_authentication_repository::JwtAuthenticationRepository,
        source_file_s3_repository::S3Repository,
        source_meta_postgres_repository::SourceMetaPostgresRepository,
        user_postgres_repository::UserPostgresRepository,
    },
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
    // rabbitmq_connection: lapin::Connection,
    // rabbitmq_queue_name_prefix: String,
    _rabbitmq_publishing_connection: Arc<lapin::Connection>,
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
    RabbitMQMessageRepositoryError(#[from] RabbitMQMessageRepositoryError),
}

impl Application {
    /// # Parameters
    /// - nb_workers: number of actix-web workers
    ///   if `None`, the number of available physical CPUs is used as the worker count.
    #[tracing::instrument(name = "Building application")]
    pub async fn build(
        settings: Settings,
        nb_workers: Option<usize>,
    ) -> Result<Self, ApplicationBuildError> {
        let connection_pool = get_connection_pool(&settings.database);

        let address = format!(
            "{}:{}",
            settings.application.host, settings.application.port
        );
        let listener = TcpListener::bind(address)?;
        let port = listener.local_addr().unwrap().port();

        let rabbitmq_publishing_connection = get_rabbitmq_connection(&settings.rabbitmq).await?;
        let rabbitmq_publishing_connection = Arc::new(rabbitmq_publishing_connection);
        let rabbitmq_content_exchange_name = format!(
            "{}_{}",
            settings.rabbitmq.exchange_name_prefix, settings.rabbitmq.content_exchange
        );

        let message_rabbitmq_repository = RabbitMQMessageRepository::new(
            rabbitmq_publishing_connection.clone(),
            &rabbitmq_content_exchange_name,
        );

        let s3_bucket = set_up_s3(&settings.object_storage).await?;
        let s3_repository = S3Repository::new(s3_bucket.clone());

        let source_meta_repository = SourceMetaPostgresRepository::new();
        let user_repository = UserPostgresRepository::new();

        let auth_repository = JwtAuthenticationRepository::new(
            settings.jwt.secret.clone(),
            settings.jwt.expire_in_s as i64,
        );

        let server = run(
            listener,
            settings,
            nb_workers,
            connection_pool,
            message_rabbitmq_repository,
            s3_repository,
            source_meta_repository,
            user_repository,
            auth_repository,
        )?;

        Ok(Self {
            server,
            port,
            s3_bucket,
            _rabbitmq_publishing_connection: rabbitmq_publishing_connection,
            // rabbitmq_connection,
            // rabbitmq_queue_name_prefix,
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
        info!("Running server ...");
        self.server.await
    }
}

/// listener: the consumer binds their own port
///
/// TracingLogger middleware: helps collecting telemetry data.
/// It generates a unique identifier for each incoming request: `request_id`.
///
/// # Parameters
/// - nb_workers: number of actix-web workers
///   if `None`, the number of available physical CPUs is used as the worker count.
pub fn run(
    listener: TcpListener,
    _settings: Settings,
    nb_workers: Option<usize>,
    db_pool: PgPool,
    message_rabbitmq_repository: RabbitMQMessageRepository,
    s3_repository: S3Repository,
    source_meta_repository: SourceMetaPostgresRepository,
    user_repository: UserPostgresRepository,
    auth_repository: JwtAuthenticationRepository,
) -> Result<Server, std::io::Error> {
    // Wraps the connection to a db in smart pointers
    let db_pool = Data::new(db_pool);

    // Wraps repositories in a `actix_web::Data` (`Arc`) to be able to register them
    // and access them from handlers.
    // Those repositories are shared among all threads.
    let s3_repository = Data::new(s3_repository);
    let source_meta_repository = Data::new(source_meta_repository);
    let user_repository = Data::new(user_repository);
    let auth_repository = Data::new(auth_repository);

    // `move` to capture variables from the surrounding environment
    let server = HttpServer::new(move || {
        info!("Starting actix-web worker");

        // Only clones thread-safe properties (ie, not the RabbitMQ channel)
        let message_rabbitmq_repository = message_rabbitmq_repository.clone();

        App::new()
            .wrap(TracingLogger::default())
            .route("/health_check", web::get().to(health_check))
            .route(
                "/add_source_files",
                web::post()
                    .to(add_source_files)
                    .wrap(RequireAuth::new(auth_repository.clone())),
            )
            .route(
                "/search",
                web::post()
                    .to(search_content)
                    .wrap(RequireAuth::new(auth_repository.clone())),
            )
            .route("/account/create", web::post().to(create_account))
            .route("/account/login", web::post().to(log_in_account))
            .app_data(db_pool.clone())
            .app_data(s3_repository.clone())
            .app_data(source_meta_repository.clone())
            .app_data(user_repository.clone())
            .app_data(auth_repository.clone())
            .data_factory(move || {
                let message_rabbitmq_repository = message_rabbitmq_repository.clone();

                async {
                    let message_rabbitmq_repository =
                        message_rabbitmq_repository.try_init().await?;

                    // Puts behind a mutex so the repository is mutable. But as the repository is cloned and then initialized inside
                    // each thread, it is not shared among all threads, and each thread mutates their own instance of the repository.
                    // The idea: a thread could re-initialize the repository if the channel is closed for ex.
                    // But is it necessary ?
                    // Ok::<Mutex<RabbitMQMessageRepository>, ApplicationBuildError>(Mutex::new(
                    //     message_rabbitmq_repository,
                    // ))
                    Ok::<RabbitMQMessageRepository, ApplicationBuildError>(
                        message_rabbitmq_repository,
                    )
                }
            })
    })
    .listen(listener)?;

    // If no workers were set, use the actix-web settings (number of workers = number of physical CPUs)
    if let Some(nb_workers) = nb_workers {
        return Ok(server.workers(nb_workers).run());
    }

    // No await
    Ok(server.run())
}

// Or should we keep a clone of the pool connection in `Application` ?
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

#[tracing::instrument(name = "Create RabbitMQ connection")]
pub async fn get_rabbitmq_connection(
    config: &RabbitMQSettings,
) -> Result<lapin::Connection, lapin::Error> {
    lapin::Connection::connect(&config.get_uri(), config.get_connection_properties()).await
}
