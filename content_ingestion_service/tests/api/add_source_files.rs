use crate::helpers::spawn_app;

#[tokio::test]
async fn add_source_files_returns_a_200_for_valid_form_data() {
    // Arrange
    let app = spawn_app().await;
    // TODO: how to fake sending a file ?
    let body = "name=le%20guin&email=ursula_le_guin%40gmail.com";

    // Act
    let response = reqwest::Client::new()
        .post(&format!("{}/add_source_files", &app.address))
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(body)
        .send()
        .await
        .expect("Failed to execute request.");

    // Assert
    assert_eq!(200, response.status().as_u16());
}
