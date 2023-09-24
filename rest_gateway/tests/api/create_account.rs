use crate::helpers::spawn_app;
use chrono::{Duration, Utc};
use rest_gateway::{
    controllers::CreateAccountBodyData, domain::entities::user_password::UserPassword,
};
use secrecy::Secret;

#[tokio::test(flavor = "multi_thread")]
async fn a_valid_user_account_should_be_created() {
    // Arranges
    let app = spawn_app().await;
    let (test_email, test_password) = app.get_test_user_credentials();

    // Acts
    let body = CreateAccountBodyData {
        email: test_email.clone(),
        password: test_password,
    };

    let response = reqwest::Client::new()
        .post(&format!("{}/account/create", &app.address))
        .json(&body)
        .send()
        .await
        .expect("Failed to execute request");

    // Asserts the API response
    assert_eq!(200, response.status().as_u16());

    // Asserts the newly created user has been persisted - only 1 user should exist
    let created_user = sqlx::query!(r#"SELECT id, email, created_at FROM users"#,)
        .fetch_one(&app.db_pool)
        .await
        .expect("Failed to fetch newly created user");

    assert_eq!(created_user.email, test_email);
    assert!(!created_user.id.is_nil());

    let five_minutes_ago = Utc::now() - Duration::minutes(5);
    assert!(created_user.created_at > five_minutes_ago);
    assert!(created_user.created_at <= Utc::now());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_valid_user_will_be_able_to_use_their_password_for_credentials() {
    // Arranges
    let app = spawn_app().await;
    let (test_email, test_password) = app.get_test_user_credentials();

    // Acts
    let body = CreateAccountBodyData {
        email: test_email.clone(),
        password: test_password,
    };

    let _response = reqwest::Client::new()
        .post(&format!("{}/account/create", &app.address))
        .json(&body)
        .send()
        .await
        .expect("Failed to execute request");

    // Asserts the newly created user has been persisted - only 1 user should exist
    let created_user = sqlx::query!(
        r#"SELECT password_hash FROM users WHERE email = $1"#,
        body.email.clone()
    )
    .fetch_one(&app.db_pool)
    .await
    .expect("Failed to fetch newly created user");

    let stored_password = Secret::new(created_user.password_hash);
    let stored_password = UserPassword::parse(&stored_password).unwrap();

    // Checks that the stored password is the same as the one used to create the account
    assert!(stored_password.verify(Secret::new(body.password)).is_ok());
}
