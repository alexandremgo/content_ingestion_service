use futures::lock::Mutex;
use std::{collections::HashMap, sync::Arc};

use content_ingestion_service::{
    domain::entities::source_meta::SourceType,
    routes::{AddSourceFilesResponse, Status},
};
use lapin::{
    message::DeliveryResult,
    options::{BasicAckOptions, BasicConsumeOptions, QueueBindOptions, QueueDeclareOptions},
    types::FieldTable,
};
use regex::Regex;
use reqwest::multipart::{Form, Part};
use tokio::time::{sleep, Duration};
use tokio_stream::StreamExt;
use tracing::{error, info, info_span, warn, Instrument};
use uuid::uuid;

use crate::helpers::{spawn_app, TestApp};

// TODO: define somewhere else
pub const EXTRACT_CONTENT_BINDING_KEY: &str = "extract_content.text.v1";

#[tokio::test(flavor = "multi_thread")]
async fn add_source_files_returns_a_200_for_valid_input_data() {
    // Arranges
    let app = spawn_app().await;
    let file_name = "example.epub";

    // Creates a multipart field (a file) from the text content
    let epub_part = Part::text("This is a test file")
        .file_name(file_name)
        .mime_str("application/epub+zip")
        .unwrap();
    let form = Form::new().part("file", epub_part);

    // Acts
    let response = reqwest::Client::new()
        .post(&format!("{}/add_source_files", &app.address))
        .multipart(form)
        .send()
        .await
        .expect("Failed to execute request");

    // Asserts
    assert_eq!(200, response.status().as_u16());

    let json_response = response.json::<AddSourceFilesResponse>().await.unwrap();
    assert_eq!(json_response.file_status.len(), 1);
    assert_eq!(
        json_response.file_status[0].file_name.as_ref().unwrap(),
        file_name
    );
    assert!(matches!(
        json_response.file_status[0].status,
        Status::Success
    ));
}

#[tokio::test(flavor = "multi_thread")]
async fn add_source_files_returns_a_400_when_input_data_is_missing() {
    // Arranges
    let app = spawn_app().await;

    // Creates a form without any multipart field
    let form = Form::new();

    // Acts
    let response = reqwest::Client::new()
        .post(&format!("{}/add_source_files", &app.address))
        .multipart(form)
        .send()
        .await
        .expect("Failed to execute request");

    // Asserts
    assert_eq!(400, response.status().as_u16());
}

#[tokio::test(flavor = "multi_thread")]
async fn add_source_files_persists_source_file_and_meta() {
    // Arranges
    let mut app = spawn_app().await;

    let counter = Arc::new(Mutex::new(0 as u32));
    listen_to_content_exchange(&mut app, EXTRACT_CONTENT_BINDING_KEY, 2000, counter.clone()).await;

    // TODO: real user
    let user_id = uuid!("f0041f88-8ad9-444f-b85a-7c522741ceae");

    let file_name = "example.epub";
    let file_content = "This is a test file";

    // Creates a multipart field (a file) from the text content
    let epub_part = Part::text(file_content)
        .file_name(file_name)
        .mime_str("application/epub+zip")
        .unwrap();
    let form = Form::new().part("file", epub_part);

    // Acts
    reqwest::Client::new()
        .post(&format!("{}/add_source_files", &app.address))
        .multipart(form)
        .send()
        .await
        .expect("Failed to execute request");

    // Asserts
    let saved = sqlx::query!(
        r#"SELECT user_id, object_store_name, source_type as "source_type: SourceType", initial_name FROM source_meta"#,
    )
    .fetch_one(&app.db_pool)
    .await
    .expect("Failed to fetch saved source file meta");

    assert_eq!(saved.initial_name, file_name);
    assert!(matches!(saved.source_type, SourceType::Epub));
    // assert!(saved.object_store_name);

    // Checks if the file has been correctly stored in the object store
    let s3_response_data = app
        .s3_bucket
        .get_object(format!("{}/{}", user_id, saved.object_store_name))
        .await
        .unwrap();

    assert_eq!(s3_response_data.to_string().unwrap(), file_content);

    // Finally asserts that the job message has been correctly sent
    let counter = counter.lock().await;
    assert_eq!(*counter, 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn add_source_files_persists_all_correct_input_source_files_and_meta_and_returns_status_for_each_file(
) {
    // Arranges
    let mut app = spawn_app().await;

    let counter = Arc::new(Mutex::new(0 as u32));
    listen_to_content_exchange(&mut app, EXTRACT_CONTENT_BINDING_KEY, 2000, counter.clone()).await;

    // TODO: real user
    let user_id = uuid!("f0041f88-8ad9-444f-b85a-7c522741ceae");
    const NUMBER_FILES: usize = 10;

    let mut form = Form::new();

    for i in 0..NUMBER_FILES {
        let file_name = format!("example_{i}.epub");
        let file_content = format!("This is the test file {i}");

        // Creates a multipart field (a file) from the text content
        let epub_part = Part::text(file_content)
            .file_name(file_name)
            .mime_str("application/epub+zip")
            .unwrap();
        form = form.part("file", epub_part);
    }

    // Acts
    let response = reqwest::Client::new()
        .post(&format!("{}/add_source_files", &app.address))
        .multipart(form)
        .send()
        .await
        .expect("Failed to execute request");

    // Asserts
    let mut files_meta_stream = sqlx::query!(
        r#"SELECT user_id, object_store_name, source_type as "source_type: SourceType", initial_name FROM source_meta"#
    )
    .fetch(&app.db_pool);

    // Going to check that the `NUMBER_FILES` files were persisted correctly with their info
    // let mut object_store_names = [Option::<&str>::None; NUMBER_FILES];
    let mut object_store_names = HashMap::<usize, String>::new();
    let re = Regex::new(r"^example_(\d+)\.epub$").unwrap();

    // Gets the associated object store name from the source meta, and checks the source type for each persisted file
    while let Ok(Some(row)) = files_meta_stream.try_next().await {
        if let Some(captures) = re.captures(&row.initial_name) {
            let i = captures.get(1).unwrap().as_str().parse::<usize>().unwrap();
            // object_store_names[i] = Some(&row.object_store_name);
            object_store_names.insert(i, row.object_store_name);

            assert!(matches!(row.source_type, SourceType::Epub));
            assert_eq!(row.user_id, user_id);
        }
    }

    // Checks that every file has been correctly stored
    for i in 0..NUMBER_FILES {
        let object_store_name = object_store_names.get(&i);

        // Their meta info was correctly saved
        assert!(object_store_name.is_some());

        let s3_response_data = app
            .s3_bucket
            .get_object(format!("{}/{}", user_id, object_store_name.unwrap()))
            .await
            .unwrap();

        assert_eq!(
            s3_response_data.to_string().unwrap(),
            format!("This is the test file {i}")
        );
    }

    // Asserts response
    assert_eq!(200, response.status().as_u16());

    let json_response = response.json::<AddSourceFilesResponse>().await.unwrap();
    assert_eq!(json_response.file_status.len(), 10);
    let mut status_checks = [false; NUMBER_FILES];

    // Gets the associated object store name from the source meta, and checks the source type for each persisted file
    for file_status in json_response.file_status {
        if let Some(captures) = re.captures(&file_status.file_name.unwrap()) {
            let i = captures.get(1).unwrap().as_str().parse::<usize>().unwrap();
            status_checks[i] = true;

            assert!(matches!(file_status.status, Status::Success));
        }
    }

    // Finally checks that all the files had a response status: Success
    for i in 0..NUMBER_FILES {
        assert!(status_checks[i]);
    }

    // Finally asserts that the job message has been correctly sent
    let counter = counter.lock().await;
    assert_eq!(*counter, NUMBER_FILES as u32);
}

/// Consumes a queue bound to the content exchange with a given binding key and increase a counter each time a message is consumed
///
/// The correct declaration of the exchange is also checked.
///
/// # Panics
/// Panics if the exchange is not declared and a queue could not bing to it after `wait_queue_timeout_ms` milliseconds
///
/// # Parameters
/// - `app`: the test app (to use and reset the rabbitmq channel)
/// - `binding_key`: the binding key to bind a generated queue to the content exchange
/// - `wait_queue_timeout_ms`: the maximum time to wait for the exchange to be declared correctly so a queue can be bound to it
/// - `counter`: the counter to increase each time a message is consumed
pub async fn listen_to_content_exchange(
    app: &mut TestApp,
    binding_key: &str,
    wait_queue_timeout_ms: usize,
    counter: Arc<Mutex<u32>>,
) {
    let mut approximate_retried_time_ms = 0;
    let retry_sleep_step_ms = 500;

    let mut queue_name = "".to_string();

    // Retries to bind a queue to the content exchange until `wait_queue_timeout_ms`
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
        if approximate_retried_time_ms > wait_queue_timeout_ms {
            panic!("Timeout: the queue {} has not been declared", queue_name);
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
            BasicConsumeOptions::default(),
            FieldTable::default(),
        )
        .await
        .unwrap();

    consumer.set_delegate(move |delivery: DeliveryResult| {
        let counter = Arc::clone(&counter);

        info!("Received message: {:?}\n", delivery);

        async move {
            let mut inner_counter = counter.lock().await;
            *inner_counter += 1;

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

            delivery
                .ack(BasicAckOptions::default())
                .await
                .expect("Failed to ack message");
        }
        .instrument(info_span!("Handling test queued message",))
    });
}
