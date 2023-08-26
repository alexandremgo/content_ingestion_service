use chrono::Utc;
use common::dtos::extracted_content::ExtractedContentDto;
use fake::{faker::lorem::en::Sentences, Fake};
use fulltext_search_service::handlers::handler_content_extracted::{queue_name, ROUTING_KEY};
use futures::lock::Mutex;
use lapin::{
    message::DeliveryResult,
    options::{BasicConsumeOptions, BasicPublishOptions, QueueBindOptions, QueueDeclareOptions},
    types::FieldTable,
    BasicProperties,
};
use serde_json::json;
use std::sync::Arc;
use tokio::time::{sleep, Duration};
use tracing::{error, info, info_span, warn, Instrument};
use uuid::Uuid;

use crate::helpers::{spawn_app, TestApp};

#[tokio::test(flavor = "multi_thread")]
async fn handler_binds_queue_to_exchange_and_acknowledges_content_extracted_message_when_correct() {
    // Arrange
    let app = spawn_app().await;
    let queue_name = queue_name(&app.rabbitmq_queue_name_prefix);

    // Checks that the service declared and bound queue to the exchange.
    // Test fails if not found after max retries.
    let queue_binding_infos = app
        .wait_until_queue_declared_and_bound_to_exchange(
            &app.rabbitmq_content_exchange_name,
            &queue_name,
            ROUTING_KEY,
            10,
        )
        .await
        .unwrap();

    info!(
        "🥦🔥 : exchange: {} binding -> {:?}",
        app.rabbitmq_content_exchange_name, queue_binding_infos
    );

    let extracted_content = ExtractedContentDto {
        id: Uuid::new_v4(),
        metadata: json!({}),
        content: Sentences(3..10).fake::<Vec<String>>().join(" "),
    };

    let message = serde_json::to_string(&extracted_content).unwrap();
    info!("Extracted content message: {}", message);

    // Sends the job message to the worker binding key
    let routing_key = ROUTING_KEY;

    app.rabbitmq_channel
        .basic_publish(
            &app.rabbitmq_content_exchange_name,
            routing_key,
            BasicPublishOptions::default(),
            message.as_bytes(),
            BasicProperties::default()
                .with_timestamp(Utc::now().timestamp_millis() as u64)
                .with_message_id(uuid::Uuid::new_v4().to_string().into()),
        )
        .await
        .unwrap();

    // Asserts that the message was acknowledged
    let max_retry = 10;
    let retry_step_time_ms = 1000;
    let mut nb_ack = 0;

    for _i in 0..max_retry {
        nb_ack = match app.get_queue_messages_stats(&queue_name).await {
            (_nb_delivered, nb_ack) => nb_ack,
        };

        if nb_ack == 1 {
            break;
        }

        sleep(Duration::from_millis(retry_step_time_ms)).await;
    }

    assert_eq!(nb_ack, 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn handler_negative_acknowledges_content_extracted_message_when_incorrect() {
    // Arrange
    let app = spawn_app().await;
    let queue_name = queue_name(&app.rabbitmq_queue_name_prefix);

    // Checks that the service declared and bound queue to the exchange.
    // Test fails if not found after max retries.
    let queue_binding_infos = app
        .wait_until_queue_declared_and_bound_to_exchange(
            &app.rabbitmq_content_exchange_name,
            &queue_name,
            ROUTING_KEY,
            10,
        )
        .await
        .unwrap();

    info!(
        "🐬🔥 : exchange: {} binding -> {:?}",
        app.rabbitmq_content_exchange_name, queue_binding_infos
    );

    let a_message_missing_metadata = json!({
        "id": Uuid::new_v4(),
        "content": Sentences(3..10).fake::<Vec<String>>().join(" "),
    });

    let message = a_message_missing_metadata.to_string();
    info!("A message missing metadata: {}", message);

    let routing_key = ROUTING_KEY;

    app.rabbitmq_channel
        .basic_publish(
            &app.rabbitmq_content_exchange_name,
            routing_key,
            BasicPublishOptions::default(),
            message.as_bytes(),
            BasicProperties::default()
                .with_timestamp(Utc::now().timestamp_millis() as u64)
                .with_message_id(uuid::Uuid::new_v4().to_string().into()),
        )
        .await
        .unwrap();

    // Asserts that the message was nack
    let max_retry = 10;
    let retry_step_time_ms = 1000;
    let mut nb_ack = 0;
    let mut nb_delivered = 0;

    for _i in 0..max_retry {
        (nb_delivered, nb_ack) = app.get_queue_messages_stats(&queue_name).await;

        if nb_ack == 0 && nb_delivered == 1 {
            break;
        }

        sleep(Duration::from_millis(retry_step_time_ms)).await;
    }

    assert_eq!(nb_delivered, 1);
    assert_eq!(nb_ack, 0);
}
