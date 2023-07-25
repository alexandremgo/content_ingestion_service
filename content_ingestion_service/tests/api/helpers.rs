use chrono::Utc;
use content_ingestion_service::{
    configuration::{get_configuration, DatabaseSettings},
    startup::{create_rabbitmq_channel, get_connection_pool, get_rabbitmq_connection, Application},
    telemetry::{get_tracing_subscriber, init_tracing_subscriber},
};
use s3::Bucket;
use sqlx::{Connection, Executor, PgConnection, PgPool};
use tracing::info;
use uuid::Uuid;

use once_cell::sync::Lazy;

// Ensures that the `tracing` stack is only initialized once using `once_cell`
static TRACING: Lazy<()> = Lazy::new(|| {
    let default_filter_level = "info".to_string();
    let subscriber_name = "test".to_string();

    // We cannot assign the output of `get_tracing_subscriber` to a variable based on the value of `TEST_LOG`
    // because the sink is part of the type returned by `get_tracing_subscriber`, therefore they are not the
    // same type. We could work around it, but this is the most straight-forward way of moving forward.
    if std::env::var("TEST_LOG").is_ok() {
        let subscriber =
            get_tracing_subscriber(subscriber_name, default_filter_level, std::io::stdout);
        init_tracing_subscriber(subscriber);
    } else {
        let subscriber =
            get_tracing_subscriber(subscriber_name, default_filter_level, std::io::sink);
        init_tracing_subscriber(subscriber);
    };
});

pub struct TestApp {
    pub address: String,
    pub port: u16,
    /// Database connection used to assert checks thanks to db queries
    pub db_pool: PgPool,
    /// S3 bucket used to assert checks thanks to requests to the S3 API
    pub s3_bucket: Bucket,
    // RabbitMQ channel used to assert checks thanks to messages sent to the queue
    rabbitmq_connection: lapin::Connection,
    pub rabbitmq_channel: lapin::Channel,
    pub rabbitmq_content_exchange_name: String,
}

/// A test API client / test suite
impl TestApp {
    pub async fn reset_rabbitmq_channel(&mut self) {
        self.rabbitmq_channel = create_rabbitmq_channel(&self.rabbitmq_connection)
            .await
            .unwrap();
    }
}

/// Launches the server as a background task
/// When a tokio runtime is shut down all tasks spawned on it are dropped.
/// tokio::test spins up a new runtime at the beginning of each test case and they shut down at the end of each test case.
/// Therefore no need to implement any clean up logic to avoid leaking resources between test runs
pub async fn spawn_app() -> TestApp {
    // The first time `initialize` is invoked the code in `TRACING` is executed.
    // All other invocations will instead skip execution.
    Lazy::force(&TRACING);

    // Randomizes configuration to ensure test isolation
    let configuration = {
        let mut c = get_configuration().expect("Failed to read configuration.");
        // Uses a different database for each test case
        c.database.database_name = format!(
            "test_{}_{}",
            Utc::now().format("%Y-%m-%d_%H-%M-%S"),
            Uuid::new_v4().to_string()
        );
        // Uses a random OS port: port 0 is special-cased at the OS level:
        // trying to bind port 0 will trigger an OS scan for an available port which will then be bound to the application.
        c.application.port = 0;

        // Using the same bucket for each integration tests, as:
        // - we cannot create an infinite number of bucket
        // - it's better to avoid creating and deleting buckets aggressively
        c.object_storage.bucket_name = "integration-tests-bucket".to_string();

        // Uses a random queue name prefix to avoid collisions between tests
        // Max size of queue name = 255 bytes
        c.rabbitmq.exchange_name_prefix = format!(
            "test_api_{}_{}",
            Utc::now().format("%Y-%m-%d_%H-%M-%S"),
            Uuid::new_v4().to_string()
        );

        c
    };

    // Creates a RabbitMQ connection (and a channel) for each test, but we did not find a way
    // to share the same connection between tests. Anyway, the Application is already recreating a connection.
    let rabbitmq_connection = get_rabbitmq_connection(&configuration.rabbitmq)
        .await
        .unwrap();
    let rabbitmq_channel = rabbitmq_connection.create_channel().await.unwrap();

    // Creates and migrates the database
    set_up_database(&configuration.database).await;

    // Only one actix-web worker is needed for integration tests
    let application = Application::build(configuration.clone(), Some(1))
        .await
        .expect("Failed to build application.");

    // Gets the port and bucket before spawning the application
    let application_port = application.port();
    let s3_bucket = application.s3_bucket();

    // Launches the application as a background task
    let _ = tokio::spawn(application.run_until_stopped());

    TestApp {
        address: format!("http://127.0.0.1:{}", application_port),
        port: application_port,
        db_pool: get_connection_pool(&configuration.database),
        s3_bucket,
        rabbitmq_connection,
        rabbitmq_channel,
        rabbitmq_content_exchange_name: format!(
            "{}_{}",
            configuration.rabbitmq.exchange_name_prefix, configuration.rabbitmq.content_exchange
        ),
    }
}

/// Creates and migrates a database for integration test
///
/// Not relying on the bash script to dynamically create databases and run migrations
async fn set_up_database(config: &DatabaseSettings) -> PgPool {
    // Creates database
    let mut connection = PgConnection::connect_with(&config.without_db())
        .await
        .expect("Failed to connect to Postgres");

    connection
        .execute(format!(r#"CREATE DATABASE "{}";"#, config.database_name).as_str())
        .await
        .expect("Failed to create database.");

    info!("🏗️  Created database: {}", config.database_name);

    let connection_pool = PgPool::connect_with(config.with_db())
        .await
        .expect("Failed to connect to Postgres.");

    // Migrates database
    sqlx::migrate!("../migrations")
        .run(&connection_pool)
        .await
        .expect("Failed to migrate the database");

    info!(
        "🏗️  Migration done for database: {} ✅",
        config.database_name
    );

    connection_pool
}
