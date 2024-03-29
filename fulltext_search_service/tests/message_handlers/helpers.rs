use std::sync::Arc;

use chrono::Utc;
use common::{
    core::rabbitmq_message_repository::RabbitMQMessageRepository,
    telemetry::{get_tracing_subscriber, init_tracing_subscriber},
};
use fulltext_search_service::{
    configuration::get_configuration,
    domain::entities::content::ContentEntity,
    startup::{get_meilisearch_client, get_rabbitmq_connection, Application},
};
use lapin::{Channel, Connection as RabbitMQConnection};
use meilisearch_sdk::{tasks::Task, Client};
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
    pub rabbitmq_connection: Arc<RabbitMQConnection>,
    pub rabbitmq_content_exchange_name: String,
    pub rabbitmq_queue_name_prefix: String,
    pub rabbitmq_management_api_config: RabbitMQManagementAPIConfig,
    // To consume messages during tests
    pub rabbitmq_channel: Channel,
    // To publish and rpc_call messages for tests
    pub rabbitmq_message_repository: RabbitMQMessageRepository,

    // To setup tests using Meilisearch
    pub meilisearch_client: Client,
    pub meilisearch_content_index: String,
}

#[derive(Debug)]
pub struct QueueBindingInfo {
    pub queue_name: String,
    pub routing_key: String,
}

impl TestApp {
    /// Helper function to wait until a queue is declared and bound to an exchange with a given routing key
    #[tracing::instrument(
        name = "Helper waiting until queue is declared and consumer is bound",
        skip(self)
    )]
    pub async fn wait_until_queue_declared_and_bound_to_exchange(
        &self,
        exchange_name: &str,
        queue_name: &str,
        routing_key: &str,
        max_retry: usize,
    ) -> Result<(), String> {
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
                    "{}/bindings/{}/e/{}/q/{}",
                    base_url, vhost, exchange_name, queue_name
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

            // The API returns an empty array if there are no bindings between the given exchange and queue
            // There could be several bindings between the queue and the exchange
            let bindings = response_json.as_array().unwrap();

            if bindings.is_empty() {
                continue;
            }

            let bound_with_routing_key = bindings.iter().find(|queue_json| {
                let binding_routing_key = queue_json["routing_key"].as_str().unwrap();

                binding_routing_key == routing_key
            });

            // The queue has not been bound with the wanted routing key to the exchange yet
            if bound_with_routing_key.is_none() {
                continue;
            }

            return Ok(());
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

    /// Re-creates a new RabbitMQ channel from the test suite RabbitMQ connection
    pub async fn reset_rabbitmq_channel(&mut self) {
        self.rabbitmq_channel = self.rabbitmq_connection.create_channel().await.unwrap();
    }

    /// Save a content to Meilisearch for tests.
    /// It waits for the task to be processed.
    pub async fn save_content_to_meilisearch(&self, content: &ContentEntity) -> Result<(), String> {
        let task = self
            .meilisearch_client
            .index(&self.meilisearch_content_index)
            .add_or_replace(&[content], None)
            .await
            .map_err(|err| {
                format!(
                    "Failed to add content to Meilisearch during tests to index {}: {}",
                    self.meilisearch_content_index, err
                )
            })?;

        let status = self
            .meilisearch_client
            .wait_for_task(task, None, None)
            .await
            .unwrap();
        assert!(matches!(status, Task::Succeeded { .. }));

        Ok(())
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

        // Meilisearch indexes can be created implicitly (when trying to add a document to an index that does not exist).
        // Using this property to isolate tests.
        c.meilisearch.contents_index = format!(
            "integration_test_index_{}_{}",
            Utc::now().format("%Y-%m-%d_%H-%M-%S"),
            Uuid::new_v4()
        );

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

    // RabbitMQ connection, channel, and message repository used by the test suite
    let rabbitmq_connection = get_rabbitmq_connection(&configuration.rabbitmq)
        .await
        .unwrap();
    let rabbitmq_connection = Arc::new(rabbitmq_connection);
    let rabbitmq_channel = rabbitmq_connection.create_channel().await.unwrap();

    let rabbitmq_content_exchange_name = format!(
        "{}_{}",
        configuration.rabbitmq.exchange_name_prefix, configuration.rabbitmq.content_exchange
    );

    let rabbitmq_message_repository = RabbitMQMessageRepository::new(
        rabbitmq_connection.clone(),
        &rabbitmq_content_exchange_name,
    );
    let rabbitmq_message_repository = rabbitmq_message_repository.try_init().await.unwrap();

    let meilisearch_client = get_meilisearch_client(&configuration.meilisearch);
    let meilisearch_content_index = configuration.meilisearch.contents_index.clone();

    tokio::spawn(application.run_until_stopped());

    info!("The application worker has been spawned into a new thread");

    TestApp {
        rabbitmq_content_exchange_name,
        rabbitmq_queue_name_prefix: configuration.rabbitmq.queue_name_prefix,
        rabbitmq_connection,
        rabbitmq_channel,
        rabbitmq_management_api_config,
        rabbitmq_message_repository,
        meilisearch_client,
        meilisearch_content_index,
    }
}
