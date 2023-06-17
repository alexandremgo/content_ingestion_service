use reqwest::multipart::{Form, Part};

use crate::helpers::spawn_app;

#[tokio::test]
async fn add_source_files_returns_a_200_for_valid_input_data() {
    // Arrange
    let app = spawn_app().await;

    // Creates a multipart field (a file) from the text content
    let epub_part = Part::text("This is a test file")
        .file_name("example.epub")
        .mime_str("application/epub+zip")
        .unwrap();
    let form = Form::new().part("file", epub_part);

    // Act
    let response = reqwest::Client::new()
        .post(&format!("{}/add_source_files", &app.address))
        .multipart(form)
        .send()
        .await
        .expect("Failed to execute request.");

    // Assert
    assert_eq!(200, response.status().as_u16());
}


#[tokio::test]
async fn add_source_files_returns_a_400_when_input_data_is_missing() {
    // Arrange
    let app = spawn_app().await;

    // Creates a form without any multipart field
    let form = Form::new();

    // Act
    let response = reqwest::Client::new()
        .post(&format!("{}/add_source_files", &app.address))
        .multipart(form)
        .send()
        .await
        .expect("Failed to execute request.");

    // Assert
    assert_eq!(400, response.status().as_u16());
}
