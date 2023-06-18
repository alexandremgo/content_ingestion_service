use std::collections::HashMap;

use content_ingestion_service::{
    domain::entities::source_meta::SourceType,
    routes::{AddSourceFilesResponse, Status},
};
use regex::Regex;
use reqwest::multipart::{Form, Part};
use tokio_stream::StreamExt;
use uuid::uuid;

use crate::helpers::spawn_app;

#[tokio::test]
async fn add_source_files_returns_a_200_for_valid_input_data() {
    // Arranges
    let app = spawn_app().await;
    let file_name = "example.epub";

    // Creates a multipart field (a file) from the text content
    let epub_part = Part::text("This is a test file")
        .file_name(file_name)
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

    let json_response = response.json::<AddSourceFilesResponse>().await.unwrap();
    assert_eq!(json_response.file_status.len(), 1);
    assert_eq!(
        json_response.file_status[0].file_name.as_ref().unwrap(),
        file_name
    );
    assert!(matches!(
        json_response.file_status[0].status,
        Status::Success
    ));
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
        r#"SELECT user_id, object_store_name, source_type as "source_type: SourceType", initial_name FROM source_meta"#,
    )
    .fetch_one(&app.db_pool)
    .await
    .expect("Failed to fetch saved source file meta");

    assert_eq!(saved.initial_name, file_name);
    assert!(matches!(saved.source_type, SourceType::Epub));
    // assert!(saved.object_store_name);

    // Checks if the file has been correctly stored in the object store
    let s3_response_data = app
        .s3_bucket
        .get_object(format!("{}/{}", user_id, saved.object_store_name))
        .await
        .unwrap();

    assert_eq!(s3_response_data.to_string().unwrap(), file_content);
}

#[tokio::test]
async fn add_source_files_persists_all_correct_input_source_files_and_meta_and_returns_status_for_each_file(
) {
    // Arranges
    let app = spawn_app().await;

    // TODO: real user
    let user_id = uuid!("f0041f88-8ad9-444f-b85a-7c522741ceae");
    const NUMBER_FILES: usize = 10;

    let mut form = Form::new();

    for i in 0..NUMBER_FILES {
        let file_name = format!("example_{i}.epub");
        let file_content = format!("This is the test file {i}");

        // Creates a multipart field (a file) from the text content
        let epub_part = Part::text(file_content)
            .file_name(file_name)
            .mime_str("application/epub+zip")
            .unwrap();
        form = form.part("file", epub_part);
    }

    // Acts
    let response = reqwest::Client::new()
        .post(&format!("{}/add_source_files", &app.address))
        .multipart(form)
        .send()
        .await
        .expect("Failed to execute request");

    // Asserts
    let mut files_meta_stream = sqlx::query!(
        r#"SELECT user_id, object_store_name, source_type as "source_type: SourceType", initial_name FROM source_meta"#
    )
    .fetch(&app.db_pool);

    // Going to check that the `NUMBER_FILES` files were persisted correctly with their info
    // let mut object_store_names = [Option::<&str>::None; NUMBER_FILES];
    let mut object_store_names = HashMap::<usize, String>::new();
    let re = Regex::new(r"^example_(\d+)\.epub$").unwrap();

    // Gets the associated object store name from the source meta, and checks the source type for each persisted file
    while let Ok(Some(row)) = files_meta_stream.try_next().await {
        if let Some(captures) = re.captures(&row.initial_name) {
            let i = captures.get(1).unwrap().as_str().parse::<usize>().unwrap();
            // object_store_names[i] = Some(&row.object_store_name);
            object_store_names.insert(i, row.object_store_name);

            assert!(matches!(row.source_type, SourceType::Epub));
            assert_eq!(row.user_id, user_id);
        }
    }

    // Checks that every file has been correctly stored
    for i in 0..NUMBER_FILES {
        let object_store_name = object_store_names.get(&i);

        // Their meta info was correctly saved
        assert!(object_store_name.is_some());

        let s3_response_data = app
            .s3_bucket
            .get_object(format!("{}/{}", user_id, object_store_name.unwrap()))
            .await
            .unwrap();

        assert_eq!(
            s3_response_data.to_string().unwrap(),
            format!("This is the test file {i}")
        );
    }

    // Asserts response
    assert_eq!(200, response.status().as_u16());

    let json_response = response.json::<AddSourceFilesResponse>().await.unwrap();
    assert_eq!(json_response.file_status.len(), 10);
    let mut status_checks = [false; NUMBER_FILES];

    // Gets the associated object store name from the source meta, and checks the source type for each persisted file
    for file_status in json_response.file_status {
        if let Some(captures) = re.captures(&file_status.file_name.unwrap()) {
            let i = captures.get(1).unwrap().as_str().parse::<usize>().unwrap();
            status_checks[i] = true;

            assert!(matches!(file_status.status, Status::Success));
        }
    }

    // Finally checks that all the files had a response status: Success
    for i in 0..NUMBER_FILES {
        assert!(status_checks[i]);
    }
}
