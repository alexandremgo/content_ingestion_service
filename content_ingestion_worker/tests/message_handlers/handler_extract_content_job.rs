use futures::lock::Mutex;
use std::sync::Arc;

use chrono::Utc;
use content_ingestion_worker::{
    domain::entities::extract_content_job::{ExtractContentJob, SourceType},
    handlers::handler_extract_content_job::BINDING_KEY,
    repositories::message_rabbitmq_repository::CONTENT_EXTRACTED_MESSAGE_KEY,
};
use lapin::{
    message::DeliveryResult,
    options::{BasicConsumeOptions, BasicPublishOptions, QueueBindOptions, QueueDeclareOptions},
    types::FieldTable,
    BasicProperties,
};
use tokio::time::{sleep, Duration};
use tracing::{error, info, info_span, warn, Instrument};
use uuid::Uuid;

use crate::helpers::{spawn_app, TestApp};

#[tokio::test(flavor = "multi_thread")]
async fn handler_binds_queue_to_exchange_and_acknowledges_extract_content_job_when_correct() {
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
        .find(|info| info.routing_key == BINDING_KEY)
        .map(|info| &info.queue_name)
        .unwrap_or_else(|| {
            panic!(
                "No queue was bound on the exchange {} with the routing key {}",
                app.rabbitmq_content_exchange_name, BINDING_KEY
            )
        });

    let job = ExtractContentJob {
        source_meta_id: Uuid::new_v4(),
        source_type: SourceType::Epub,
        object_store_path_name: format!("{}/{}", Uuid::new_v4(), "test.epub"),
        source_initial_name: "test.epub".to_string(),
    };

    // Adding the associated test file to the S3 bucket
    let file_name = "sample_3_chapters.epub";
    let file_path_name = format!("tests/resources/{}", file_name);
    app.save_file_to_s3_bucket(&file_path_name, &job.object_store_path_name)
        .await
        .unwrap();

    let job = serde_json::to_string(&job).unwrap();

    // Sends the job message to the worker binding key
    let routing_key = BINDING_KEY;

    app.rabbitmq_channel
        .basic_publish(
            &app.rabbitmq_content_exchange_name,
            routing_key,
            BasicPublishOptions::default(),
            job.as_bytes(),
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
async fn handler_negative_acknowledges_extract_content_job_when_file_not_in_s3() {
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
        .find(|info| info.routing_key == BINDING_KEY)
        .map(|info| &info.queue_name)
        .unwrap_or_else(|| {
            panic!(
                "No queue was bound on the exchange {} with the routing key {}",
                app.rabbitmq_content_exchange_name, BINDING_KEY
            )
        });

    let job = ExtractContentJob {
        source_meta_id: Uuid::new_v4(),
        source_type: SourceType::Epub,
        object_store_path_name: format!("{}/{}", Uuid::new_v4(), "test.epub"),
        source_initial_name: "test.epub".to_string(),
    };
    let job = serde_json::to_string(&job).unwrap();

    let routing_key = BINDING_KEY;

    app.rabbitmq_channel
        .basic_publish(
            &app.rabbitmq_content_exchange_name,
            routing_key,
            BasicPublishOptions::default(),
            job.as_bytes(),
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

#[tokio::test(flavor = "multi_thread")]
async fn handler_publishes_extract_contented_on_correct_job() {
    // Arrange
    let mut app = spawn_app().await;

    // Already checked that the worker binds a queue to the exchange correctly,
    // but we need to wait that the worker is fully setup.
    app.wait_until_queues_declared_and_bound_to_exchange(&app.rabbitmq_content_exchange_name, 10)
        .await
        .unwrap();

    let counter = Arc::new(Mutex::new(0_u32));
    listen_to_content_exchange(
        &mut app,
        CONTENT_EXTRACTED_MESSAGE_KEY,
        2000,
        counter.clone(),
    )
    .await;

    let job = ExtractContentJob {
        source_meta_id: Uuid::new_v4(),
        source_type: SourceType::Epub,
        object_store_path_name: format!("{}/{}", Uuid::new_v4(), "test.epub"),
        source_initial_name: "test.epub".to_string(),
    };

    // Adding the associated test file to the S3 bucket
    let file_name = "sample_3_chapters.epub";
    let file_path_name = format!("tests/resources/{}", file_name);
    app.save_file_to_s3_bucket(&file_path_name, &job.object_store_path_name)
        .await
        .unwrap();

    let job = serde_json::to_string(&job).unwrap();

    // Sends the job message to the worker binding key
    let routing_key = BINDING_KEY;

    app.rabbitmq_channel
        .basic_publish(
            &app.rabbitmq_content_exchange_name,
            routing_key,
            BasicPublishOptions::default(),
            job.as_bytes(),
            BasicProperties::default()
                .with_timestamp(Utc::now().timestamp_millis() as u64)
                .with_message_id(uuid::Uuid::new_v4().to_string().into()),
        )
        .await
        .unwrap();

    let mut approximate_retried_time_ms = 0;
    let retry_sleep_step_ms = 500;
    let timeout_extracted_content_ms = 30000;
    let expected_number_extracted_contents = 3;
    let mut number_extracted_contents = 0;

    // Asserts that the extracted content are being published
    // TODO: checks actual extracted contents ?
    loop {
        let counter = counter.lock().await;

        if *counter >= expected_number_extracted_contents {
            number_extracted_contents = *counter;
            break;
        }
        drop(counter);

        info!("No extracted contents ... sleeping");
        approximate_retried_time_ms += retry_sleep_step_ms;
        if approximate_retried_time_ms > timeout_extracted_content_ms {
            panic!(
                "Timeout: did not received enough extracted content listening to {}: {}",
                CONTENT_EXTRACTED_MESSAGE_KEY, 0
            );
        }

        sleep(Duration::from_millis(retry_sleep_step_ms as u64)).await;
    }

    assert_eq!(
        number_extracted_contents,
        expected_number_extracted_contents
    );
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
/// - `binding_key`: the binding key to bind a generated queue to the content exchange
/// - `timeout_binding_exchange_ms`: the maximum time to wait for the exchange to be declared correctly so a queue can be bound to it
/// - `counter`: the counter to increase each time a message is consumed
pub async fn listen_to_content_exchange(
    app: &mut TestApp,
    binding_key: &str,
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
                binding_key,
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
                &app.rabbitmq_content_exchange_name, binding_key
            );
        }

        sleep(Duration::from_millis(retry_sleep_step_ms as u64)).await;
    }

    info!(
        "Declared queue {} on exchange {}, binding on {}",
        queue_name, app.rabbitmq_content_exchange_name, binding_key
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
