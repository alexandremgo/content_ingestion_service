use content_ingestion_worker::{
    configuration::get_configuration,
    handlers::example::MyData,
    startup::Application,
    telemetry::{get_tracing_subscriber, init_tracing_subscriber},
};
use lapin::{options::BasicPublishOptions, BasicProperties};

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

    let queue_name = format!("{}_queue_test", &configuration.rabbitmq.queue_name_prefix);

    let app = Application::build(configuration).await.unwrap();
    let channel = app.create_rabbitmq_channel().await;
    let _result = app.run().await;

    loop {
        let my_data = MyData {
            field_1: "test".to_string(),
            field_2: "ok".to_string(),
        };
        let my_data = serde_json::to_string(&my_data).unwrap();
        let current_time_ms = chrono::Utc::now().timestamp_millis() as u64;

        channel
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

        tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
    }
}
