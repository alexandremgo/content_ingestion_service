use std::sync::Arc;

use chrono::Utc;
use common::core::rabbitmq_message_repository::{
    RabbitMQMessageRepository, RabbitMQMessageRepositoryError,
};
use futures::StreamExt;
use tracing::info;
use uuid::Uuid;

use crate::helpers::init_test;
use lapin::{
    options::{BasicAckOptions, BasicConsumeOptions, QueueBindOptions, QueueDeclareOptions},
    types::FieldTable,
    Connection, ConnectionProperties,
};

#[tokio::test(flavor = "multi_thread")]
async fn rpc_call_should_get_a_response() {
    init_test();

    // RabbitMQ connection, channel, and message repository used by the test suite
    let connection = get_rabbitmq_connection("127.0.0.1", "5672").await.unwrap();
    let connection = Arc::new(connection);
    let channel = connection.create_channel().await.unwrap();

    let exchange_name = format!(
        "test_exchange_{}_{}",
        Utc::now().format("%Y-%m-%d_%H-%M-%S"),
        Uuid::new_v4()
    );
    let queue_name = format!(
        "test_queue_{}_{}",
        Utc::now().format("%Y-%m-%d_%H-%M-%S"),
        Uuid::new_v4()
    );
    let routing_key = "test.v1";

    // Creates and inits message repository for the tested publisher
    let rabbitmq_message_repository = RabbitMQMessageRepository::new(connection, &exchange_name);
    // The inits declares the exchange if it was not declared before
    let rabbitmq_message_repository = rabbitmq_message_repository.try_init().await.unwrap();

    // A separate message repository for the consumer thread
    let consumer_rabbitmq_message_repository = rabbitmq_message_repository.clone();

    // Declares and binds the test consumer queue outside the spawned thread so the test waits for it to be fully setup
    channel
        .queue_declare(
            &queue_name,
            QueueDeclareOptions::default(),
            FieldTable::default(),
        )
        .await
        .unwrap();
    channel
        .queue_bind(
            &queue_name,
            &exchange_name,
            routing_key,
            QueueBindOptions::default(),
            FieldTable::default(),
        )
        .await
        .unwrap();

    info!(
        "Declared test queue {} on exchange {}, binding on {}",
        queue_name, exchange_name, routing_key,
    );

    let expected_request = String::from("request_test");
    let request_message = expected_request.clone();
    let expected_response = String::from("response_test");
    let response_message = expected_response.clone();

    // Consumer of the RPC call that will send back a response
    tokio::spawn(async move {
        // Inits the message repository for the test consumer
        let consumer_rabbitmq_message_repository = consumer_rabbitmq_message_repository
            .try_init()
            .await
            .unwrap();

        let consumer_options = BasicConsumeOptions {
            no_ack: false,
            ..BasicConsumeOptions::default()
        };
        let mut consumer = channel
            .basic_consume(&queue_name, "", consumer_options, FieldTable::default())
            .await
            .unwrap();

        while let Some(delivery) = consumer.next().await {
            async {
                let delivery = match delivery {
                    Ok(delivery) => delivery,
                    Err(error) => {
                        panic!(
                            "Failed to consume queue message on queue {}: {}",
                            queue_name, error
                        );
                    }
                };

                let request = std::str::from_utf8(&delivery.data).unwrap();
                assert_eq!(request, expected_request);

                let reply_to = match delivery.properties.reply_to() {
                    Some(reply_to) => reply_to.to_string(),
                    None => panic!("No reply-to property from RPC call message"),
                };

                info!(
                    ?request,
                    ?reply_to,
                    "Received request message, responding..."
                );

                // Sends response to the given `reply_to` to mimic a RPC call
                consumer_rabbitmq_message_repository
                    .rpc_respond(&reply_to, response_message.as_bytes())
                    .await
                    .unwrap();

                if let Err(error) = delivery.ack(BasicAckOptions::default()).await {
                    panic!("Failed to ack extract_content_job message: {:?}", error);
                }
            }
            .await
        }
    });

    let response = rabbitmq_message_repository
        .rpc_call(routing_key, request_message.as_bytes(), None)
        .await
        .unwrap();
    let response = String::from_utf8(response).unwrap();

    info!("RPC call response: {}", response);
    assert_eq!(response, expected_response);
}

#[tokio::test(flavor = "multi_thread")]
async fn rpc_call_should_timeout_if_no_response() {
    init_test();

    // RabbitMQ connection, channel, and message repository used by the test suite
    let connection = get_rabbitmq_connection("127.0.0.1", "5672").await.unwrap();
    let connection = Arc::new(connection);
    let channel = connection.create_channel().await.unwrap();

    let exchange_name = format!(
        "test_exchange_{}_{}",
        Utc::now().format("%Y-%m-%d_%H-%M-%S"),
        Uuid::new_v4()
    );
    let queue_name = format!(
        "test_queue_{}_{}",
        Utc::now().format("%Y-%m-%d_%H-%M-%S"),
        Uuid::new_v4()
    );
    let routing_key = "test.v1";

    // Creates and inits message repository for the tested publisher
    let rabbitmq_message_repository = RabbitMQMessageRepository::new(connection, &exchange_name);
    // The inits declares the exchange if it was not declared before
    let rabbitmq_message_repository = rabbitmq_message_repository.try_init().await.unwrap();

    // Declares and binds the test consumer queue outside the spawned thread so the test waits for it to be fully setup
    channel
        .queue_declare(
            &queue_name,
            QueueDeclareOptions::default(),
            FieldTable::default(),
        )
        .await
        .unwrap();
    channel
        .queue_bind(
            &queue_name,
            &exchange_name,
            routing_key,
            QueueBindOptions::default(),
            FieldTable::default(),
        )
        .await
        .unwrap();

    info!(
        "Declared test queue {} on exchange {}, binding on {}",
        queue_name, exchange_name, routing_key,
    );

    let expected_request = String::from("request_test");
    let request_message = expected_request.clone();

    // Consumer of the RPC call that will send back a response
    tokio::spawn(async move {
        let consumer_options = BasicConsumeOptions {
            no_ack: false,
            ..BasicConsumeOptions::default()
        };
        let mut consumer = channel
            .basic_consume(&queue_name, "", consumer_options, FieldTable::default())
            .await
            .unwrap();

        while let Some(delivery) = consumer.next().await {
            async {
                let delivery = match delivery {
                    Ok(delivery) => delivery,
                    Err(error) => {
                        panic!(
                            "Failed to consume queue message on queue {}: {}",
                            queue_name, error
                        );
                    }
                };

                let request = std::str::from_utf8(&delivery.data).unwrap();
                assert_eq!(request, expected_request);

                let reply_to = match delivery.properties.reply_to() {
                    Some(reply_to) => reply_to.to_string(),
                    None => panic!("No reply-to property from RPC call message"),
                };

                info!(
                    ?request,
                    ?reply_to,
                    "Received request message, NOT responding to"
                );

                // NOT responding to it. Just acked.
                if let Err(error) = delivery.ack(BasicAckOptions::default()).await {
                    panic!("Failed to ack extract_content_job message: {:?}", error);
                }
            }
            .await
        }
    });

    let response = rabbitmq_message_repository
        .rpc_call(routing_key, request_message.as_bytes(), Some(1000))
        .await;

    assert!(matches!(
        response,
        Err(RabbitMQMessageRepositoryError::Timeout(_))
    ));
}

/// Create a connection to RabbitMQ
pub async fn get_rabbitmq_connection(host: &str, port: &str) -> Result<Connection, lapin::Error> {
    let connection_properties = ConnectionProperties::default()
        // Use tokio executor and reactor.
        // At the moment the reactor is only available for unix.
        .with_executor(tokio_executor_trait::Tokio::current())
        .with_reactor(tokio_reactor_trait::Tokio);

    let uri = format!("amqp://{}:{}", host, port);
    Connection::connect(&uri, connection_properties).await
}
