use actix_web::{
    dev::Server,
    web::{self, Data},
    App, HttpServer,
};
use sqlx::{postgres::PgPoolOptions, PgPool};
use std::net::TcpListener;
use tracing_actix_web::TracingLogger;

use crate::{
    configuration::{DatabaseSettings, Settings},
    repositories::{
        source_file_s3_repository::S3Repository,
        source_meta_postgres_repository::{self, SourceMetaPostgresRepository},
    },
    routes::{add_source_files, health_check},
};

/// Holds the newly built server and its port
pub struct Application {
    port: u16,
    server: Server,
}

#[derive(thiserror::Error, Debug)]
pub enum ApplicationBuildError {
    #[error(transparent)]
    IOError(#[from] std::io::Error),
}

impl Application {
    pub async fn build(configuration: Settings) -> Result<Self, ApplicationBuildError> {
        let connection_pool = get_connection_pool(&configuration.database);

        let address = format!(
            "{}:{}",
            configuration.application.host, configuration.application.port
        );
        let listener = TcpListener::bind(address)?;
        let port = listener.local_addr().unwrap().port();

        let s3_repository = S3Repository::new(&configuration.object_storage);
        let source_meta_repository = SourceMetaPostgresRepository::new();

        let server = run(
            listener,
            connection_pool,
            s3_repository,
            source_meta_repository,
        )?;

        Ok(Self { port, server })
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    /// This function only returns when the application is stopped
    pub async fn run_until_stopped(self) -> Result<(), std::io::Error> {
        self.server.await
    }
}

/// listener: the consumer binds their own port
///
/// TracingLogger middleware: helps collecting telemetry data.
/// It generates a unique identifier for each incoming request: `request_id`.
pub fn run(
    listener: TcpListener,
    db_pool: PgPool,
    s3_repository: S3Repository,
    source_meta_repository: SourceMetaPostgresRepository,
) -> Result<Server, std::io::Error> {
    // Wraps the connection to a db in smart pointers
    let db_pool = Data::new(db_pool);

    // Wraps repositories to register them and access them from handlers
    let s3_repository = Data::new(s3_repository);
    let source_meta_repository = Data::new(source_meta_repository);

    // `move` to capture `connection` from the surrounding environment
    let server = HttpServer::new(move || {
        App::new()
            .wrap(TracingLogger::default())
            .route("/health_check", web::get().to(health_check))
            .route("/add_source_files", web::post().to(add_source_files))
            // .route("/ingest_document", web::post().to(publish_newsletter))
            // Registers the db connection as part of the application state
            // Gets a pointer copy and attach it to the application state
            .app_data(db_pool.clone())
            .app_data(s3_repository.clone())
            .app_data(source_meta_repository.clone())
    })
    .listen(listener)?
    .run();

    // No await
    Ok(server)
}

pub fn get_connection_pool(configuration: &DatabaseSettings) -> PgPool {
    PgPoolOptions::new()
        .acquire_timeout(std::time::Duration::from_secs(2))
        .connect_lazy_with(configuration.with_db())
}
