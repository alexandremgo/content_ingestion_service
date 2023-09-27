use std::io::Read;

use chrono::Utc;
use common::telemetry::{get_tracing_subscriber, init_tracing_subscriber};
use content_ingestion_worker::{
    configuration::get_configuration,
    startup::{get_rabbitmq_connection, Application},
};
use lapin::{Channel, Connection as RabbitMQConnection};
use s3::Bucket;
use tokio::time::{sleep, Duration};
use tracing::info;
use uuid::Uuid;

use once_cell::sync::Lazy;

// Ensures that the `tracing` stack is only initialized once using `once_cell`
static TRACING: Lazy<()> = Lazy::new(|| {
    let default_filter_level = "info".to_string();
    let subscriber_name = "message_handlers_tests".to_string();

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
    pub rabbitmq_connection: RabbitMQConnection,
    pub rabbitmq_content_exchange_name: String,
    pub rabbitmq_management_api_config: RabbitMQManagementAPIConfig,
    pub rabbitmq_channel: Channel,

    /// S3 bucket used to setup tests thanks to requests to the S3 API
    pub s3_bucket: Bucket,
}

#[derive(Debug)]
pub struct QueueBindingInfo {
    pub queue_name: String,
    pub routing_key: String,
}

impl TestApp {
    /// Helper function to wait until some queues are declared and bound to the given exchange
    ///
    /// # Returns
    /// A Result containing a list of `QueueBindingInfo` if no issue occurred, or an error string message
    #[tracing::instrument(
        name = "Helper waiting until queue is declared and consumer is bound",
        skip(self)
    )]
    pub async fn wait_until_queues_declared_and_bound_to_exchange(
        &self,
        exchange_name: &str,
        max_retry: usize,
    ) -> Result<Vec<QueueBindingInfo>, String> {
        let client = reqwest::Client::new();

        let retry_step_time_ms = 1000;

        let RabbitMQManagementAPIConfig {
            base_url,
            username,
            password,
            vhost,
        } = &self.rabbitmq_management_api_config;

        for _i in 0..max_retry {
            sleep(Duration::from_millis(retry_step_time_ms)).await;

            let response = client
                .get(&format!(
                    "{}/exchanges/{}/{}/bindings/source",
                    base_url, vhost, exchange_name
                ))
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
                    exchange_name, err
                )
            })?;

            info!("📡 From management API: response {:?}", response_json);

            // The API returns an empty array if there are no bindings on the given exchange
            let bound_queues = response_json.as_array().unwrap();

            if bound_queues.is_empty() {
                continue;
            }

            let queue_binding_infos = bound_queues
                .iter()
                .map(|queue_json| {
                    let queue_name = queue_json["destination"].as_str().unwrap().to_string();
                    let routing_key = queue_json["routing_key"].as_str().unwrap().to_string();

                    QueueBindingInfo {
                        queue_name,
                        routing_key,
                    }
                })
                .collect();

            return Ok(queue_binding_infos);
        }

        Err(format!(
            "❌ no queues were declared and bound to {}",
            self.rabbitmq_content_exchange_name
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

    /// Helper function to save a str as an object in the S3 bucket
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

    /// Helper function to save a `File` in the S3 bucket
    ///
    /// # Params
    /// - `file_path_name`: The local path of the file to save
    /// - `object_path_name`: The path of the object in the S3 bucket
    pub async fn save_file_to_s3_bucket(
        &self,
        file_path_name: &str,
        object_path_name: &str,
    ) -> Result<(), String> {
        let mut file = std::fs::File::open(file_path_name).unwrap();
        let mut buf = Vec::<u8>::new();
        file.read_to_end(&mut buf)
            .map_err(|err| format!("Could not read file {}: {}", object_path_name, err))?;

        self.s3_bucket
            .put_object(object_path_name.clone(), buf.as_slice())
            .await
            .map_err(|err| {
                format!(
                    "S3 failed to add file as an object in {}: {}",
                    object_path_name, err
                )
            })?;

        Ok(())
    }

    /// Re-creates a new RabbitMQ channel from the test suite RabbitMQ connection
    pub async fn reset_rabbitmq_channel(&mut self) {
        self.rabbitmq_channel = self.rabbitmq_connection.create_channel().await.unwrap();
    }
}

/// Launches the worker/server/RabbitMQ connection as a background task
pub async fn spawn_app() -> TestApp {
    // The first time `initialize` is invoked the code in `TRACING` is executed.
    // All other invocations will instead skip execution.
    Lazy::force(&TRACING);

    // Randomizes configuration to ensure test isolation: random prefix for our queue names
    let configuration = {
        let mut c = get_configuration().expect("Failed to read configuration.");

        // Uses a different exchange and queue names for each test case
        c.rabbitmq.exchange_name_prefix = format!(
            "test_{}_{}_{}",
            c.rabbitmq.exchange_name_prefix,
            Utc::now().format("%Y-%m-%d_%H-%M-%S"),
            Uuid::new_v4()
        );
        c.rabbitmq.queue_name_prefix = format!(
            "test_{}_{}_{}",
            c.rabbitmq.queue_name_prefix,
            Utc::now().format("%Y-%m-%d_%H-%M-%S"),
            Uuid::new_v4()
        );

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

    // RabbitMQ connection used by the test suite
    let rabbitmq_connection = get_rabbitmq_connection(&configuration.rabbitmq)
        .await
        .unwrap();
    let rabbitmq_channel = rabbitmq_connection.create_channel().await.unwrap();

    tokio::spawn(application.run_until_stopped());

    info!("The application worker has been spawned into a new thread");

    TestApp {
        rabbitmq_content_exchange_name: format!(
            "{}_{}",
            configuration.rabbitmq.exchange_name_prefix, configuration.rabbitmq.content_exchange
        ),
        rabbitmq_connection,
        rabbitmq_channel,
        rabbitmq_management_api_config,
        s3_bucket,
    }
}
