use content_ingestion_service::{
    configuration::{get_configuration, DatabaseSettings},
    startup::{get_connection_pool, Application},
    telemetry::{get_tracing_subscriber, init_tracing_subscriber},
};
use sqlx::{Connection, Executor, PgConnection, PgPool};
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
    pub db_pool: PgPool,
    pub port: u16,
}

/// A test API client / test suite
impl TestApp {
    // /// Sends a POST request to the "/subscriptions" route
    // pub async fn post_subscriptions(&self, body: String) -> reqwest::Response {
    //     reqwest::Client::new()
    //         .post(&format!("{}/subscriptions", &self.address))
    //         .header("Content-Type", "application/x-www-form-urlencoded")
    //         .body(body)
    //         .send()
    //         .await
    //         .expect("Failed to execute request.")
    // }
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
        c.database.database_name = Uuid::new_v4().to_string();
        // Uses a random OS port: port 0 is special-cased at the OS level:
        // trying to bind port 0 will trigger an OS scan for an available port which will then be bound to the application.
        c.application.port = 0;

        c
    };

    // Creates and migrates the database
    configure_database(&configuration.database).await;

    let application = Application::build(configuration.clone())
        .await
        .expect("Failed to build application.");
    // Gets the port before spawning the application
    let application_port = application.port();
    // Launches the application as a background task
    let _ = tokio::spawn(application.run_until_stopped());

    TestApp {
        address: format!("http://127.0.0.1:{}", application_port),
        port: application_port,
        db_pool: get_connection_pool(&configuration.database),
    }
}

async fn configure_database(config: &DatabaseSettings) -> PgPool {
    // Creates database
    let mut connection = PgConnection::connect_with(&config.without_db())
        .await
        .expect("Failed to connect to Postgres");

    connection
        .execute(format!(r#"CREATE DATABASE "{}";"#, config.database_name).as_str())
        .await
        .expect("Failed to create database.");

    // Migrates database
    let connection_pool = PgPool::connect_with(config.with_db())
        .await
        .expect("Failed to connect to Postgres.");

    // TODO
    // sqlx::migrate!("./migrations")
    //     .run(&connection_pool)
    //     .await
    //     .expect("Failed to migrate the database");

    connection_pool
}