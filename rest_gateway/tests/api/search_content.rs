use tracing::info;

use crate::helpers::spawn_app;

#[tokio::test(flavor = "multi_thread")]
async fn search_content_returns_a_response() {
    let app = spawn_app().await;

    let body = serde_json::json!({
        "query": "test"
    });

    let response = reqwest::Client::new()
        .post(&format!("{}/search", &app.address))
        .json(&body)
        .send()
        .await
        .expect("Failed to execute request.");

    info!("Response: {:?}", response);

    assert!(response.status().is_success());
    assert_eq!(Some(0), response.content_length());
}
