use crate::domain::entities::source_meta::{SourceMeta, SourceType};
use crate::middlewares::jwt_authentication::middleware::UserIdFromToken;
use crate::repositories::source_file_s3_repository::S3Repository;
use crate::repositories::source_meta_postgres_repository::SourceMetaPostgresRepository;
use actix_multipart::form::{tempfile::TempFile, MultipartForm};
use actix_web::http::StatusCode;
use actix_web::{web, HttpResponse, ResponseError};
use anyhow::Context;
use common::constants::routing_keys::EXTRACT_CONTENT_TEXT_ROUTING_KEY;
use common::core::rabbitmq_message_repository::RabbitMQMessageRepository;
use common::dtos::extract_content_job::ExtractContentJobDto;
use common::helper::error_chain_fmt;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use std::path::Path;
use std::str::FromStr;
use tracing::{error, info};

#[derive(Debug, MultipartForm)]
pub struct UploadForm {
    #[multipart(rename = "file")]
    files: Vec<TempFile>,
}

#[derive(thiserror::Error)]
pub enum AddSourceFilesError {
    #[error("No source files were uploaded")]
    NoSourceFiles,
    #[error("{0}")]
    RepositoryAccessError(String),
    #[error(transparent)]
    UnexpectedError(#[from] anyhow::Error),
    #[error("Error while serializing message data: {0}")]
    JsonError(#[from] serde_json::Error),
}

impl std::fmt::Debug for AddSourceFilesError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        error_chain_fmt(self, f)
    }
}

impl ResponseError for AddSourceFilesError {
    fn status_code(&self) -> StatusCode {
        match self {
            AddSourceFilesError::UnexpectedError(_)
            | AddSourceFilesError::RepositoryAccessError(_)
            | AddSourceFilesError::JsonError(_) => StatusCode::INTERNAL_SERVER_ERROR,
            AddSourceFilesError::NoSourceFiles => StatusCode::BAD_REQUEST,
        }
    }
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Success,
    Error,
}

#[derive(Serialize, Deserialize)]
pub struct AddSourceFileStatus {
    pub file_name: Option<String>,
    pub status: Status,
    pub message: Option<String>,
}

#[derive(Serialize, Deserialize)]
pub struct AddSourceFilesResponse {
    pub file_status: Vec<AddSourceFileStatus>,
}

/// Add source files to the object storage for a user
#[tracing::instrument(
    name = "Add source files",
    skip(
        form,
        pool,
        s3_repository,
        source_meta_repository,
        message_rabbitmq_repository
    ),
    err
)]
pub async fn add_source_files(
    MultipartForm(mut form): MultipartForm<UploadForm>,
    pool: web::Data<PgPool>,
    s3_repository: web::Data<S3Repository>,
    source_meta_repository: web::Data<SourceMetaPostgresRepository>,
    message_rabbitmq_repository: web::Data<RabbitMQMessageRepository>,
    user_id: web::ReqData<UserIdFromToken>,
) -> Result<HttpResponse, AddSourceFilesError> {
    let user_id = user_id.into_inner().0;
    info!("Request for user_id: {}", user_id);

    let mut response = AddSourceFilesResponse {
        file_status: Vec::new(),
    };

    if form.files.is_empty() {
        return Err(AddSourceFilesError::NoSourceFiles);
    }

    for (idx, temp_file) in form.files.iter_mut().enumerate() {
        // 1. Parsing step

        // File name coming from the HTTP Content-Disposition header:
        // In a multipart/form-data body, HTTP Content-Disposition is a header that must be used
        // on each subpart of a multipart body to give information about the field it applies to.
        let file_name = match temp_file.file_name.clone() {
            Some(filename) => filename,
            None => {
                error!("{}: no file name", idx);
                // Goes to the next one if there is no name
                response.file_status.push(AddSourceFileStatus {
                    file_name: None,
                    status: Status::Error,
                    message: Some("No file name".to_string()),
                });
                continue;
            }
        };
        let extension = match Path::new(&file_name).extension() {
            Some(extension) => match extension.to_str() {
                Some(extension) => extension,
                None => {
                    error!(
                        "{}: no unicode representation for the extension of {}",
                        idx, file_name
                    );

                    response.file_status.push(AddSourceFileStatus {
                        file_name: Some(file_name),
                        status: Status::Error,
                        message: Some("No unicode representation for the extension".to_string()),
                    });
                    continue;
                }
            },
            None => {
                error!("{}: could not extract extension from {}", idx, file_name);

                response.file_status.push(AddSourceFileStatus {
                    file_name: Some(file_name),
                    status: Status::Error,
                    message: Some("Could not extract extension".to_string()),
                });
                continue;
            }
        };
        let bytes_size = temp_file.size;

        let source_type = match SourceType::from_str(extension) {
            Ok(source_type) => source_type,
            Err(error) => {
                error!("{}: Invalid source type for {}: {}", idx, file_name, error);

                response.file_status.push(AddSourceFileStatus {
                    file_name: Some(file_name),
                    status: Status::Error,
                    message: Some("Invalid source type for {}".to_string()),
                });
                continue;
            }
        };

        info!(
            "Saving file {}, of size {} and of type {:?}",
            file_name, bytes_size, source_type,
        );

        // 2. Storing step
        let mut transaction = pool
            .begin()
            .await
            .context("Failed to acquire a Postgres connection from the pool")?;

        let (object_name, object_path_name) = s3_repository
            .save_file(&user_id.to_string(), temp_file.file.as_file_mut())
            .await
            .context(format!(
                "The file {} could not be uploaded to object storage",
                file_name
            ))?;

        let source_meta = SourceMeta::builder()
            .user_id(user_id.to_owned())
            .initial_name(file_name.clone())
            .source_type(SourceType::Epub)
            .object_store_name(object_name.clone())
            .build();

        source_meta_repository
            .add_source_meta(&mut transaction, &source_meta)
            .await
            .context(format!(
                "Could not save the file information of {}",
                file_name
            ))?;

        transaction.commit().await.context(format!(
            "Failed to commit SQL transaction to store the file {}",
            file_name
        ))?;

        // TODO: Rolls back on error to avoid storing unused file
        // // Removes file if problem when saving file/object info
        // s3_repository
        //     .remove_file_from_bucket(&bucket, &object_name)
        //     .await
        //     .context(format!(
        //         "The object {} could not be removed from the object storage",
        //         object_name
        //     ))?;

        let job = ExtractContentJobDto {
            source_meta_id: source_meta.id,
            source_type: source_type.into(),
            object_store_path_name: object_path_name,
            source_initial_name: file_name.clone(),
        };

        let json_job = serde_json::to_string(&job)?;

        message_rabbitmq_repository
            .publish(EXTRACT_CONTENT_TEXT_ROUTING_KEY, json_job.as_bytes())
            .await
            .context(format!(
                "Could not send content extraction job request for the file {}",
                file_name
            ))?;

        response.file_status.push(AddSourceFileStatus {
            file_name: Some(file_name),
            status: Status::Success,
            message: None,
        });
    }

    Ok(HttpResponse::Ok().json(response))
}
