use content_ingestion_worker::{
    configuration::get_configuration,
    domain::entities::extract_content_job::{ExtractContentJob, SourceType},
    repositories::message_rabbitmq_repository::CONTENT_EXTRACT_JOB_QUEUE,
    startup::Application,
    telemetry::{get_tracing_subscriber, init_tracing_subscriber},
};
use lapin::{
    options::{BasicPublishOptions, ConfirmSelectOptions},
    BasicProperties,
};
use tracing::info;
use uuid::Uuid;

#[tokio::main]
async fn main() {
    let tracing_subscriber = get_tracing_subscriber(
        "content_ingestion_worker".into(),
        "info".into(),
        std::io::stdout,
    );
    init_tracing_subscriber(tracing_subscriber);

    // Panics if the configuration can't be read
    let configuration = get_configuration().expect("Failed to read configuration.");

    let queue_name = format!(
        "{}_{}",
        &configuration.rabbitmq.queue_name_prefix, CONTENT_EXTRACT_JOB_QUEUE
    );

    let app = Application::build(configuration).await.unwrap();
    let channel = app.create_rabbitmq_channel().await.unwrap();

    loop {
        let job = ExtractContentJob {
            source_meta_id: Uuid::new_v4(),
            source_type: SourceType::Epub,
            object_store_path_name: format!("{}/{}", Uuid::new_v4(), "test.epub"),
        };
        let job = serde_json::to_string(&job).unwrap();

        let current_time_ms = chrono::Utc::now().timestamp_millis() as u64;

        let confirmation = channel
            .basic_publish(
                "",
                &queue_name,
                BasicPublishOptions::default(),
                &job.as_bytes(),
                BasicProperties::default()
                    .with_timestamp(current_time_ms)
                    .with_message_id(uuid::Uuid::new_v4().to_string().into()),
            )
            .await
            .unwrap();
        // .await
        // .unwrap();

        info!("🥦 Confirmation on {}: {:?}", queue_name, confirmation);

        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;

        let management_api_url = "http://localhost:15672/api";
        let username = "guest";
        let password = "guest";
        let vhost = "%2F"; // URL-encoded vhost, use "%2F" for the default vhost

        // let queue_name = "your_queue_name";

        let client = reqwest::Client::new();
        let response = client
            .get(&format!(
                "{}/queues/{}/{}",
                management_api_url, vhost, queue_name
            ))
            .basic_auth(username, Some(password))
            .send()
            .await
            .expect("🚨 could not send a request to RabbitMQ management API");

        let response_json: serde_json::Value = response.json().await.unwrap();
        let message_stats = &response_json["message_stats"];
        let unacknowledged = &response_json["messages_unacknowledged"]
            .as_u64()
            .unwrap_or(0);
        info!("🕵️  YOLO: response json: {:?}", response_json);
        info!("🥦 unacknowledged: {:?}", unacknowledged);

        let acknowledged = message_stats["ack"].as_u64().unwrap_or(0);
        let delivered = message_stats["deliver"].as_u64().unwrap_or(0);
        let messages_ready = message_stats["ready"].as_u64().unwrap_or(0);

        info!("🕵️  ✅ Acknowledged messages: {}", acknowledged);
        info!("🕵️  Delivered: {}", delivered);
        info!("🕵️  Messages ready for delivery: {}", messages_ready);

        tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
    }
}
