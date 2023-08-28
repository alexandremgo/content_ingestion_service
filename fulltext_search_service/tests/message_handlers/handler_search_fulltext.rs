use common::dtos::{
    fulltext_search_request::FulltextSearchRequestDto,
    fulltext_search_response::FulltextSearchResponseDto, templates::rpc_response::RpcErrorStatus,
};
use fake::{faker::lorem::en::Sentences, Fake};
use fulltext_search_service::handlers::handler_search_fulltext::{queue_name, ROUTING_KEY};
use serde_json::json;
use tokio::time::{sleep, Duration};
use tracing::info;
use uuid::Uuid;

use crate::helpers::spawn_app;

#[tokio::test(flavor = "multi_thread")]
async fn handler_binds_queue_to_exchange_and_acknowledges_search_fulltext_request_when_correct() {
    // Arrange
    let app = spawn_app().await;
    let queue_name = queue_name(&app.rabbitmq_queue_name_prefix);

    // Checks that the service declared and bound queue to the exchange.
    // Test fails if not found after max retries.
    app.wait_until_queue_declared_and_bound_to_exchange(
        &app.rabbitmq_content_exchange_name,
        &queue_name,
        ROUTING_KEY,
        10,
    )
    .await
    .unwrap();

    let search_request = FulltextSearchRequestDto {
        id: Uuid::new_v4(),
        metadata: json!({}),
        content: Sentences(3..10).fake::<Vec<String>>().join(" "),
    };
    let search_request = serde_json::to_string(&search_request).unwrap();
    info!("Fulltext Search request message: {}", search_request);

    // Sends the job message to the worker binding key
    let routing_key = ROUTING_KEY;

    let response = app
        .rabbitmq_message_repository
        .rpc_call(routing_key, search_request.as_bytes(), None)
        .await
        .unwrap();

    let response = FulltextSearchResponseDto::try_parsing(&response).unwrap();
    info!("Fulltext Search response: {:?}", response);
    assert!(matches!(response, FulltextSearchResponseDto::Ok { .. }));

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
async fn handler_returns_error_response_on_incorrect_search_fulltext_request_and_nacks() {
    // Arrange
    let app = spawn_app().await;
    let queue_name = queue_name(&app.rabbitmq_queue_name_prefix);

    // Checks that the service declared and bound queue to the exchange.
    // Test fails if not found after max retries.
    app.wait_until_queue_declared_and_bound_to_exchange(
        &app.rabbitmq_content_exchange_name,
        &queue_name,
        ROUTING_KEY,
        10,
    )
    .await
    .unwrap();

    let a_request_missing_metadata = json!({
        "id": Uuid::new_v4(),
        "content": Sentences(3..10).fake::<Vec<String>>().join(" "),
    });
    let a_request_missing_metadata = a_request_missing_metadata.to_string();
    info!("A request missing metadata: {}", a_request_missing_metadata);

    let routing_key = ROUTING_KEY;

    let response = app
        .rabbitmq_message_repository
        .rpc_call(routing_key, a_request_missing_metadata.as_bytes(), None)
        .await
        .unwrap();

    let response = FulltextSearchResponseDto::try_parsing(&response).unwrap();
    info!("Fulltext Search response: {:?}", response);
    assert!(matches!(
        response,
        FulltextSearchResponseDto::Error {
            status: RpcErrorStatus::BadRequest,
            ..
        }
    ));

    // Asserts that the message was nacked
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