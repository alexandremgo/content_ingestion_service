use content_ingestion_worker::{
    helper::error_chain_fmt,
    telemetry::{get_tracing_subscriber, init_tracing_subscriber}, configuration::get_configuration,
};
use lapin::{
    message::DeliveryResult,
    options::{BasicAckOptions, BasicConsumeOptions, BasicPublishOptions, QueueDeclareOptions},
    types::FieldTable,
    BasicProperties, Connection, ConnectionProperties,
};
use tracing::{error, info, info_span, Instrument};

#[derive(serde::Serialize, serde::Deserialize, Debug)]
struct MyData {
    field_1: String,
    field_2: String,
}

#[derive(thiserror::Error)]
pub enum MyDataParsingError {
    #[error("Data could not be converted from utf8 u8 vector to string")]
    InvalidStringData(#[from] std::str::Utf8Error),

    #[error("Data did not represent a valid JSON object: {0}")]
    InvalidJsonData(#[from] serde_json::Error),
}

impl std::fmt::Debug for MyDataParsingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        error_chain_fmt(self, f)
    }
}

impl MyData {
    fn try_parsing(data: &Vec<u8>) -> Result<Self, MyDataParsingError> {
        let data = std::str::from_utf8(data)?;
        let my_data = serde_json::from_str(data)?;

        Ok(my_data)
    }
}

#[tracing::instrument(name = "Handling queued job")]
fn handler(my_data: MyData) -> Result<(), String> {
    // Do something with the delivery data (The message payload)
    info!("🍕 Received data: {:?}\n", my_data);

    Ok(())
}

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

    let connection = Connection::connect(&configuration.rabbitmq.get_uri(), configuration.rabbitmq.get_connection_properties()).await.unwrap();
    let channel = connection.create_channel().await.unwrap();

    let _queue = channel
        .queue_declare(
            "queue_test",
            QueueDeclareOptions::default(),
            FieldTable::default(),
        )
        .await
        .unwrap();

    let consumer = channel
        .basic_consume(
            "queue_test",
            "tag_foo",
            BasicConsumeOptions::default(),
            FieldTable::default(),
        )
        .await
        .unwrap();

    consumer.set_delegate(move |delivery: DeliveryResult| {
        async move {
            let delivery = match delivery {
                // Carries the delivery alongside its channel
                Ok(Some(delivery)) => delivery,
                // The consumer got canceled
                Ok(None) => return,
                // Carries the error and is always followed by Ok(None)
                Err(error) => {
                    error!(?error, "Failed to consume queue message");
                    return;
                }
            };

            let my_data = match MyData::try_parsing(&delivery.data) {
                Ok(my_data) => my_data,
                Err(error) => {
                    error!(?error, "Failed to parse queue message data: {}", error);
                    return;
                }
            };

            info!(
                "🦖 Received message properties: {:#?}\n",
                delivery.properties
            );

            match handler(my_data) {
                Ok(()) => (),
                Err(error) => {
                    error!(?error, "Failed to handle queue message");
                    return;
                }
            }

            delivery
                .ack(BasicAckOptions::default())
                .await
                .expect("Failed to ack send_webhook_event message");
        }
        .instrument(info_span!(
            "Handling queued message",
            handler_id = %uuid::Uuid::new_v4()
        ))
    });

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
                "queue_test",
                BasicPublishOptions::default(),
                &my_data.as_bytes(),
                BasicProperties::default()
                    .with_timestamp(current_time_ms)
                    .with_message_id(uuid::Uuid::new_v4().to_string().into())
            )
            .await
            .unwrap()
            .await
            .unwrap();

        tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
    }
}
