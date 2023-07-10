use chrono::Utc;
use content_ingestion_worker::{
    domain::entities::extract_content_job::{ExtractContentJob, SourceType},
    repositories::message_rabbitmq_repository::CONTENT_EXTRACT_JOB_QUEUE,
};
use lapin::{options::BasicPublishOptions, BasicProperties};
use tokio::time::{sleep, Duration};
use uuid::Uuid;

use crate::helpers::spawn_app;

#[tokio::test(flavor = "multi_thread")]
async fn handler_acknowledges_extract_content_job_when_correct() {
    // Arrange
    let app = spawn_app().await;

    let queue_name = format!(
        "{}_{}",
        app.rabbitmq_queue_name_prefix, CONTENT_EXTRACT_JOB_QUEUE
    );

    // Checks that the worker declared and bound a consumer to the queue
    // If this fails, the test fails
    app.wait_until_declared_queue_and_bound_consumer(&queue_name, 10)
        .await
        .unwrap();

    let job = ExtractContentJob {
        source_meta_id: Uuid::new_v4(),
        source_type: SourceType::Epub,
        object_store_path_name: format!("{}/{}", Uuid::new_v4(), "test.epub"),
    };

    // Adding the associated fake file to the S3 bucket
    let file_name = "sample_3_chapters.epub";
    let file_path_name = format!("tests/resources/{}", file_name);
    app.save_file_to_s3_bucket(&file_path_name, &job.object_store_path_name)
        .await
        .unwrap();

    let job = serde_json::to_string(&job).unwrap();

    app.rabbitmq_channel
        .basic_publish(
            "",
            &queue_name,
            BasicPublishOptions::default(),
            &job.as_bytes(),
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

    app.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn handler_negative_acknowledges_extract_content_job_when_file_not_in_s3() {
    // Arrange
    let app = spawn_app().await;

    let queue_name = format!(
        "{}_{}",
        app.rabbitmq_queue_name_prefix, CONTENT_EXTRACT_JOB_QUEUE
    );

    // Checks that the worker declared and bound a consumer to the queue
    // If this fails, the test fails
    app.wait_until_declared_queue_and_bound_consumer(&queue_name, 10)
        .await
        .unwrap();

    let job = ExtractContentJob {
        source_meta_id: Uuid::new_v4(),
        source_type: SourceType::Epub,
        object_store_path_name: format!("{}/{}", Uuid::new_v4(), "test.epub"),
    };
    let job = serde_json::to_string(&job).unwrap();

    app.rabbitmq_channel
        .basic_publish(
            "",
            &queue_name,
            BasicPublishOptions::default(),
            &job.as_bytes(),
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

    app.shutdown().await;
}
