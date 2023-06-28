use content_ingestion_worker::{
    configuration::get_configuration,
    handlers::example::MyData,
    startup::Application,
    telemetry::{get_tracing_subscriber, init_tracing_subscriber},
};
use lapin::{options::BasicPublishOptions, BasicProperties, Channel};
use s3::Bucket;
use tokio::time::{sleep, Duration};
use tokio_util::sync::CancellationToken;
use tracing::info;
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

/// Represents the configuration for the RabbitMQ management API
///
/// The RabbitMQ management API is only used in integration tests
pub struct RabbitMQManagementAPIConfig {
    pub base_url: String,
    pub password: String,
    pub username: String,
    pub vhost: String,
}

/// A test API client
///
/// A test suite to easily create integration tests
pub struct TestApp {
    pub rabbitmq_queue_name_prefix: String,
    pub rabbitmq_channel: Channel,
    cancel_token: CancellationToken,
    pub rabbitmq_management_api_config: RabbitMQManagementAPIConfig,

    /// S3 bucket used to setup tests thanks to requests to the S3 API
    pub s3_bucket: Bucket,
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

    /// Helper function to wait until the queue is declared and a consumer is bound to it
    ///
    /// # Returns
    /// A Result containing an error message if an issue occurred
    #[tracing::instrument(
        name = "Helper waiting until queue is declared and consumer is bound",
        skip(self)
    )]
    pub async fn wait_until_declared_queue_and_bound_consumer(
        &self,
        queue_name: &str,
        max_retry: usize,
    ) -> Result<(), String> {
        let client = reqwest::Client::new();

        let retry_step_time_ms = 1000;
        let mut declared_and_bound = false;

        let RabbitMQManagementAPIConfig {
            base_url,
            username,
            password,
            vhost,
        } = &self.rabbitmq_management_api_config;

        for _i in 0..max_retry {
            sleep(Duration::from_millis(retry_step_time_ms)).await;

            let response = client
                .get(&format!("{}/queues/{}/{}", base_url, vhost, queue_name))
                .basic_auth(username, Some(password))
                .send()
                .await
                .map_err(|err| {
                    format!(
                        "🚨 could not send a request to RabbitMQ management API: {:?}",
                        err
                    )
                })?;

            // Continues on queue not found
            if response.status() == 404 {
                continue;
            }

            let response_json: serde_json::Value = response.json().await.map_err(|err| {
                format!(
                    "❌ could not deserialize the response from RabbitMQ management API on {}: {}",
                    queue_name, err
                )
            })?;
            let nb_consumers = response_json["consumers"].as_u64().unwrap_or(0);

            info!(
                "📡 From management API: nb consumers for {} = {:?}",
                queue_name, nb_consumers
            );

            if nb_consumers < 1 {
                continue;
            } else {
                declared_and_bound = true;
                break;
            }
        }

        if declared_and_bound {
            return Ok(());
        }

        Err(format!(
            "❌ the queue {} was not declared and/or no consumers was bound to it",
            queue_name
        ))
    }

    /// Helper function fetching the monitored number of delivered and acknowledged messages
    ///
    /// # Returns
    /// A tuple with: (nb of messages delivered, nb of messages acknowledged)
    #[tracing::instrument(name = "Helper get queue messages stats", skip(self))]
    pub async fn get_queue_messages_stats(&self, queue_name: &str) -> (u64, u64) {
        let RabbitMQManagementAPIConfig {
            base_url,
            username,
            password,
            vhost,
        } = &self.rabbitmq_management_api_config;

        let client = reqwest::Client::new();
        let response = client
            .get(&format!("{}/queues/{}/{}", base_url, vhost, queue_name))
            .basic_auth(username, Some(password))
            .send()
            .await
            .expect("🚨 could not send a request to RabbitMQ management API");

        let response_json: serde_json::Value = response.json().await.unwrap();
        let message_stats = &response_json["message_stats"];

        let nb_ack = message_stats["ack"].as_u64().unwrap_or(0);
        let total_delivered = message_stats["deliver"].as_u64().unwrap_or(0);

        info!(
            "📡 From management API on {}: total_delivered = {}, acknowledged = {}",
            queue_name, total_delivered, nb_ack
        );

        (total_delivered, nb_ack)
    }

    pub async fn save_content_to_s3_bucket(
        &self,
        content: &str,
        object_path_name: &str,
    ) -> Result<(), String> {
        let content_bytes = content.as_bytes();

        self.s3_bucket
            .put_object(object_path_name.clone(), content_bytes)
            .await
            .map_err(|err| {
                format!(
                    "S3 failed to add content as an object in {}: {}",
                    object_path_name, err
                )
            })?;

        Ok(())
    }
    /// Shutdowns the test suite by sending a cancel signal to every registered spawned tasks (the RabbitMQ client/worker app)
    ///
    /// It was needed because the spawned RabbitMQ client/worker app was not shutting down correctly after each test
    pub async fn shutdown(self) {
        self.cancel_token.cancel();
    }
}

/// Launches the worker/server/RabbitMQ connection as a background task
///
/// Note: When a tokio runtime is shut down all tasks spawned on it are dropped.
/// tokio::test spins up a new runtime at the beginning of each test case and they shut down at the end of each test case.
/// Therefore normally there is no need to implement any clean up logic to avoid leaking resources between test runs
/// But the RabbitMQ worker is not being shutdown gracefully...
pub async fn spawn_app() -> TestApp {
    // The first time `initialize` is invoked the code in `TRACING` is executed.
    // All other invocations will instead skip execution.
    Lazy::force(&TRACING);

    // Randomizes configuration to ensure test isolation: random prefix for our queue names
    let configuration = {
        let mut c = get_configuration().expect("Failed to read configuration.");

        // Uses a different queue for each test case
        c.rabbitmq.queue_name_prefix = Uuid::new_v4().to_string();

        // Using the same bucket for each integration tests, as:
        // - we cannot create an infinite number of bucket
        // - it's better to avoid creating and deleting buckets aggressively
        // - on github action: it is created when initializing the workflow (with the aws cli)
        //   to avoid concurrent tests trying to create the same bucket at the same time
        c.object_storage.bucket_name = "integration-tests-bucket".to_string();

        c
    };

    let rabbitmq_management_api_config = RabbitMQManagementAPIConfig {
        base_url: "http://localhost:15672/api".to_string(),
        username: "guest".to_string(),
        password: "guest".to_string(),
        vhost: "%2F".to_string(), // URL-encoded vhost, use "%2F" for the default vhost
    };

    let application = Application::build(configuration.clone())
        .await
        .expect("Failed to build application.");

    // Gets the S3 bucket before spawning the application
    let s3_bucket = application.s3_bucket();

    // Creates a RabbitMQ channel before `application` is moved
    let channel = application.create_rabbitmq_channel().await.unwrap();

    // To force the shutdown of the application running as an infinite loop
    let cancel_token = CancellationToken::new();
    let cloned_cancel_token = cancel_token.clone();

    tokio::spawn(application.run_until_stopped(cloned_cancel_token));

    TestApp {
        rabbitmq_queue_name_prefix: configuration.rabbitmq.queue_name_prefix,
        rabbitmq_channel: channel,
        cancel_token,
        rabbitmq_management_api_config,
        s3_bucket,
    }
}
