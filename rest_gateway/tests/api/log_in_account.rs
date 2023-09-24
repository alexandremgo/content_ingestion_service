use crate::helpers::spawn_app;
use rest_gateway::controllers::{LogInAccountBodyData, LogInAccountResponse};

#[tokio::test(flavor = "multi_thread")]
async fn a_valid_user_account_should_get_a_valid_access_token() {
    // Arranges
    let app = spawn_app().await;
    let (test_user_id, test_email, test_password) = app.create_test_user_account().await;

    // Acts
    let body = LogInAccountBodyData {
        email: test_email.clone(),
        password: test_password,
    };

    let response = reqwest::Client::new()
        .post(&format!("{}/account/login", &app.address))
        .json(&body)
        .send()
        .await
        .expect("Failed to execute request");

    // Asserts the API response
    assert_eq!(200, response.status().as_u16());

    let json_response = response.json::<LogInAccountResponse>().await.unwrap();
    let access_token = json_response.access_token;
    assert!(!access_token.is_empty());

    let token_user_id = app.decode_access_token(&access_token);
    assert_eq!(token_user_id, test_user_id);
}
