use common::{
    constants::routing_keys::SEARCH_FULLTEXT_ROUTING_KEY,
    dtos::fulltext_search_response::{FulltextSearchResponseData, FulltextSearchResponseDto},
};
use reqwest::header::{HeaderValue, AUTHORIZATION};
use tracing::info;

use crate::helpers::spawn_app;

#[tokio::test(flavor = "multi_thread")]
async fn search_content_returns_a_response_from_a_valid_request() {
    let mut app = spawn_app().await;

    // Fake user and access token
    let (_, token) = app.get_test_user_token();

    // Sets up a fake response from the search service
    let fake_response = FulltextSearchResponseDto::Ok {
        data: FulltextSearchResponseData { results: vec![] },
    };
    let fake_response = fake_response.try_serializing().unwrap();

    app.listen_and_respond_from_rpc(
        SEARCH_FULLTEXT_ROUTING_KEY,
        5000,
        Vec::from(fake_response.as_bytes()),
    )
    .await;

    // Acts
    let body = serde_json::json!({
        "query": "test"
    });

    let response = reqwest::Client::new()
        .post(&format!("{}/search", &app.address))
        .header(
            AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {}", token)).unwrap(),
        )
        .json(&body)
        .send()
        .await
        .expect("Failed to execute request.");

    info!("Response: {:?}", response);

    // Asserts
    assert!(response.status().is_success());
}
