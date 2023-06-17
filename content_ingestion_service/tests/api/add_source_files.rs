use reqwest::multipart::{Form, Part};
use uuid::uuid;

use crate::helpers::spawn_app;

#[tokio::test]
async fn add_source_files_returns_a_200_for_valid_input_data() {
    // Arranges
    let app = spawn_app().await;

    // Creates a multipart field (a file) from the text content
    let epub_part = Part::text("This is a test file")
        .file_name("example.epub")
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
}

#[tokio::test]
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

#[tokio::test]
async fn add_source_files_persists_source_file_and_meta() {
    // Arranges
    let app = spawn_app().await;

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
        "SELECT user_id, object_store_name, source_type, initial_name FROM source_meta",
    )
    .fetch_one(&app.db_pool)
    .await
    .expect("Failed to fetch saved source file meta");

    assert_eq!(saved.initial_name, file_name);
    assert_eq!(saved.source_type, "epub");
    // assert!(saved.object_store_name);

    // Checks if the file has been correctly stored in the object store
    let s3_response_data = app
        .s3_bucket
        .get_object(format!("{}/{}", user_id, saved.object_store_name))
        .await
        .unwrap();

    assert_eq!(s3_response_data.to_string().unwrap(), file_content);
}
