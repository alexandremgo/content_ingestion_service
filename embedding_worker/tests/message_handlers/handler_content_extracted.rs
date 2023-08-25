use chrono::Utc;
use embedding_worker::{
    domain::entities::extracted_content::ExtractedContent,
    handlers::handler_content_extracted::ROUTING_KEY,
};
use fake::{faker::lorem::en::Sentences, Fake};
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

    // Checks that the worker declared and bind a queue to the exchange
    // If this fails, the test fails
    let queue_binding_infos = app
        .wait_until_queues_declared_and_bound_to_exchange(&app.rabbitmq_content_exchange_name, 10)
        .await
        .unwrap();

    let queue_name = queue_binding_infos
        .iter()
        .find(|info| info.routing_key == ROUTING_KEY)
        .map(|info| &info.queue_name)
        .unwrap_or_else(|| {
            panic!(
                "No queue was bound on the exchange {} with the routing key {}",
                app.rabbitmq_content_exchange_name, ROUTING_KEY
            )
        });

    let extracted_content = ExtractedContent {
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
        nb_ack = match app.get_queue_messages_stats(queue_name).await {
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

    // Checks that the worker declared and bind a queue to the exchange
    // If this fails, the test fails
    let queue_binding_infos = app
        .wait_until_queues_declared_and_bound_to_exchange(&app.rabbitmq_content_exchange_name, 10)
        .await
        .unwrap();

    let queue_name = queue_binding_infos
        .iter()
        .find(|info| info.routing_key == ROUTING_KEY)
        .map(|info| &info.queue_name)
        .unwrap_or_else(|| {
            panic!(
                "No queue was bound on the exchange {} with the routing key {}",
                app.rabbitmq_content_exchange_name, ROUTING_KEY
            )
        });

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
        (nb_delivered, nb_ack) = app.get_queue_messages_stats(queue_name).await;

        if nb_ack == 0 && nb_delivered == 1 {
            break;
        }

        sleep(Duration::from_millis(retry_step_time_ms)).await;
    }

    assert_eq!(nb_delivered, 1);
    assert_eq!(nb_ack, 0);
}

/// Consumes messages from a queue bound to the content exchange with a given binding key
/// and increase a counter each time a message is consumed
///
/// The correct declaration of the exchange is also checked.
///
/// # Panics
/// Panics if the exchange is not declared and a queue could not bing to it after `timeout_binding_exchange_ms` milliseconds
///
/// # Parameters
/// - `app`: the test app (to use and reset the rabbitmq channel)
/// - `routing_key`: the binding key to bind a generated queue to the content exchange
/// - `timeout_binding_exchange_ms`: the maximum time to wait for the exchange to be declared correctly so a queue can be bound to it
/// - `counter`: the counter to increase each time a message is consumed
pub async fn listen_to_content_exchange(
    app: &mut TestApp,
    routing_key: &str,
    timeout_binding_exchange_ms: usize,
    counter: Arc<Mutex<u32>>,
) {
    let mut approximate_retried_time_ms = 0;
    let retry_sleep_step_ms = 500;

    let mut queue_name = "".to_string();

    // Retries to bind a queue to the content exchange until `timeout_binding_exchange_ms`
    loop {
        // When supplying an empty string queue name, RabbitMQ generates a name for us, returned from the queue declaration request
        let queue = app
            .rabbitmq_channel
            .queue_declare("", QueueDeclareOptions::default(), FieldTable::default())
            .await
            .unwrap();

        match app
            .rabbitmq_channel
            .queue_bind(
                queue.name().as_str(),
                &app.rabbitmq_content_exchange_name,
                routing_key,
                QueueBindOptions::default(),
                FieldTable::default(),
            )
            .await
        {
            Ok(_) => {
                queue_name = queue.name().as_str().to_owned();
                break;
            }
            Err(error) => match error {
                lapin::Error::ProtocolError(_) | lapin::Error::InvalidChannelState(_) => {
                    warn!(
                        "RabbitMQ queue error: queue {} does not exist, retrying ...",
                        queue_name
                    );
                    // When the queue does not exist, the channel is closed
                    app.reset_rabbitmq_channel().await;
                }
                _ => {
                    panic!(
                        "Unknown error while checking for the RabbitMQ queue {:?}",
                        queue_name
                    );
                }
            },
        };

        approximate_retried_time_ms += retry_sleep_step_ms;
        if approximate_retried_time_ms > timeout_binding_exchange_ms {
            panic!(
                "Timeout: could not bind a queue to the exchange {} with the binding key {}",
                &app.rabbitmq_content_exchange_name, routing_key
            );
        }

        sleep(Duration::from_millis(retry_sleep_step_ms as u64)).await;
    }

    info!(
        "Declared queue {} on exchange {}, binding on {}",
        queue_name, app.rabbitmq_content_exchange_name, routing_key
    );

    let consumer = app
        .rabbitmq_channel
        .basic_consume(
            &queue_name,
            "",
            BasicConsumeOptions {
                no_ack: true,
                ..BasicConsumeOptions::default()
            },
            FieldTable::default(),
        )
        .await
        .unwrap();

    consumer.set_delegate(move |delivery: DeliveryResult| {
        let counter = Arc::clone(&counter);

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

            info!("Received message: {:?}\n", delivery);

            let mut inner_counter = counter.lock().await;
            *inner_counter += 1;
        }
        .instrument(info_span!("Handling test queued message",))
    });
}
