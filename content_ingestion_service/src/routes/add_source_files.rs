use crate::{helper::error_chain_fmt, repositories::source_file_s3_repository::S3Repository};
use actix_multipart::form::{tempfile::TempFile, MultipartForm};
use actix_web::http::StatusCode;
use actix_web::{web, HttpResponse, ResponseError};
use anyhow::Context;
use tracing::info;

#[derive(Debug, MultipartForm)]
pub struct UploadForm {
    #[multipart(rename = "file")]
    files: Vec<TempFile>,
}

#[derive(thiserror::Error)]
pub enum AddSourceFilesError {
    #[error(transparent)]
    UnexpectedError(#[from] anyhow::Error),
}

impl std::fmt::Debug for AddSourceFilesError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        error_chain_fmt(self, f)
    }
}

impl ResponseError for AddSourceFilesError {
    fn status_code(&self) -> StatusCode {
        match self {
            AddSourceFilesError::UnexpectedError(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}

/// Add source files to the object storage for a user
#[tracing::instrument(name = "Add source files", skip(form, s3_repository))]
pub async fn add_source_files(
    MultipartForm(form): MultipartForm<UploadForm>,
    s3_repository: web::Data<S3Repository>,
) -> Result<HttpResponse, AddSourceFilesError> {
    // TODO: real user
    let user_id = "test-user2";

    let bucket = s3_repository
        .get_or_create_bucket(user_id)
        .await
        .context(format!(
            "Failed to create a storage bucket for the user {}",
            user_id
        ))?;

    for mut temp_file in form.files {
        info!("Adding file: {:?} ...", temp_file.file_name);
        let file_name = temp_file
            .file_name
            .clone()
            .unwrap_or("unknown_file_name".to_string());

        let object_name = s3_repository
            .save_file_to_bucket(&bucket, temp_file.file.as_file_mut())
            .await
            .context(format!(
                "The file {} could not be uploaded to object storage",
                file_name
            ))?;

        info!("Added file: {:?} -> {:?}", temp_file.file_name, object_name);

        // TODO: remove file if problem when saving file/object info
        // s3_repository
        //     .remove_file_from_bucket(&bucket, &object_name)
        //     .await
        //     .context(format!(
        //         "The object {} could not be removed from the object storage",
        //         object_name
        //     ))?;

        // info!("Deleted object: {:?}", object_name);
    }

    Ok(HttpResponse::Ok().finish())
}
