use crate::helpers::spawn_app;

#[tokio::test(flavor = "multi_thread")]
async fn health_check_works() {
    let app = spawn_app().await;

    // Performs HTTP requests against our application
    let client = reqwest::Client::new();

    let response = client
        .get(&format!("{}/health_check", &app.address))
        .send()
        .await
        .expect("Failed to execute request");

    assert!(response.status().is_success());
    assert_eq!(Some(0), response.content_length());
}
