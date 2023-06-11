use crate::{
    helper::error_chain_fmt, repositories::source_file_s3_repository::get_or_create_bucket,
};
use actix_multipart::form::{tempfile::TempFile, MultipartForm};
use actix_web::http::StatusCode;
use actix_web::{HttpResponse, ResponseError};
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
#[tracing::instrument(name = "Add source files", skip(form))]
pub async fn add_source_files(
    MultipartForm(form): MultipartForm<UploadForm>,
) -> Result<HttpResponse, AddSourceFilesError> {
    // TODO: real user
    let user_id = "test-user2";

    get_or_create_bucket(user_id).await.context(format!(
        "Failed to create a storage bucket for the user {}",
        user_id
    ))?;

    for file in form.files {
        info!("Adding file: {:?}", file.file_name);
        // let path = format!("./tmp/{}", f.file_name.unwrap());
        // log::info!("saving to {path}");
        // f.file.persist(path).unwrap();
    }

    Ok(HttpResponse::Ok().finish())
}
