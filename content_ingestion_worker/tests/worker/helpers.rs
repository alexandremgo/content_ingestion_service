use content_ingestion_worker::{
    configuration::get_configuration,
    handlers::example::MyData,
    startup::Application,
    telemetry::{get_tracing_subscriber, init_tracing_subscriber},
};
use lapin::{options::BasicPublishOptions, BasicProperties, Channel};
use uuid::Uuid;

use once_cell::sync::Lazy;

// Ensures that the `tracing` stack is only initialized once using `once_cell`
static TRACING: Lazy<()> = Lazy::new(|| {
    let default_filter_level = "info".to_string();
    let subscriber_name = "test".to_string();

    // We cannot assign the output of `get_tracing_subscriber` to a variable based on the value of `TEST_LOG`
    // because the sink is part of the type returned by `get_tracing_subscriber`, therefore they are not the
    // same type. The easiest is to have 2 code branches: one with `stdout`, and one `sink`.
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

/// A test API client
///
/// A test suite to easily create integration tests
pub struct TestApp {
    application: Application,
    rabbitmq_queue_name_prefix: String,
    rabbitmq_channel: Channel,
}

impl TestApp {
    /// Sends an `example` message
    pub async fn send_queue_message(&self, my_data: MyData) -> Result<(), ()> {
        let my_data = serde_json::to_string(&my_data).unwrap();
        let current_time_ms = chrono::Utc::now().timestamp_millis() as u64;

        let queue_name = format!("{}_queue_test", &self.rabbitmq_queue_name_prefix);

        self.rabbitmq_channel
            .basic_publish(
                "",
                &queue_name,
                BasicPublishOptions::default(),
                &my_data.as_bytes(),
                BasicProperties::default()
                    .with_timestamp(current_time_ms)
                    .with_message_id(uuid::Uuid::new_v4().to_string().into()),
            )
            .await
            .unwrap()
            .await
            .unwrap();

        Ok(())
    }
}

/// Launches the worker/server/RabbitMQ connection as a background task
///
/// When a tokio runtime is shut down all tasks spawned on it are dropped.
/// tokio::test spins up a new runtime at the beginning of each test case and they shut down at the end of each test case.
/// Therefore no need to implement any clean up logic to avoid leaking resources between test runs
/// prepare_app() ?
pub async fn spawn_app() -> TestApp {
    // The first time `initialize` is invoked the code in `TRACING` is executed.
    // All other invocations will instead skip execution.
    Lazy::force(&TRACING);

    // Randomizes configuration to ensure test isolation: random prefix for our queue names
    let configuration = {
        let mut c = get_configuration().expect("Failed to read configuration.");

        // Uses a different queue for each test case
        c.rabbitmq.queue_name_prefix = Uuid::new_v4().to_string();

        c
    };

    let application = Application::build(configuration.clone())
        .await
        .expect("Failed to build application.");
    // Creates a RabbitMQ channel before `application` is moved
    let channel = application.create_rabbitmq_channel().await.clone();

    // Launches the application as a background task
    // TODO: but how do we access the application ?
    // HERE: Actually lets first check that lapin can give us access to the current number of message in a queue
    // use message_count on queue ? https://docs.rs/lapin/latest/lapin/struct.Queue.html#method.message_count

    // let _ = tokio::spawn(application.run_until_stopped());

    TestApp {
        application,
        rabbitmq_queue_name_prefix: configuration.rabbitmq.queue_name_prefix,
        // rabbitmq_channel: "totot".to_string(),
        rabbitmq_channel: channel,
    }
}
