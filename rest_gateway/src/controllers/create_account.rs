use actix_web::http::header::ContentType;
use actix_web::http::StatusCode;
use actix_web::{web, HttpResponse, ResponseError};
use anyhow::Context;
use common::helper::error_chain_fmt;
use secrecy::Secret;
use serde_json::json;
use sqlx::PgPool;
use tracing::info;

use crate::domain::entities::user::UserError;
use crate::repositories::user_postgres_repository::UserPostgresRepositoryError;
use crate::{
    domain::entities::user::User, repositories::user_postgres_repository::UserPostgresRepository,
};

#[tracing::instrument(name = "Create user account", skip(pool, user_repository, body))]
pub async fn create_account(
    pool: web::Data<PgPool>,
    user_repository: web::Data<UserPostgresRepository>,
    body: web::Json<CreateAccountBodyData>,
) -> Result<HttpResponse, CreateAccountError> {
    info!(email = body.email, "Creating account");

    let user = User::create(&body.email, Secret::new(body.password.clone()))
        .await
        .map_err(|error| match error {
            UserError::EmailError(_) => CreateAccountError::InvalidEmail(body.email.clone()),
            UserError::PasswordError(e) => CreateAccountError::InvalidPassword(format!("{}", e)),
            UserError::InternalError(_) | UserError::InvalidCredentials(_) => {
                CreateAccountError::InternalError(error.into())
            }
        })?;

    let mut transaction = pool
        .begin()
        .await
        .context("Failed to acquire a Postgres connection from the pool")?;

    user_repository.add_user(&mut transaction, &user).await?;

    transaction.commit().await.context(format!(
        "Failed to commit SQL transaction to store new user {}",
        body.email,
    ))?;

    info!(email = body.email, "Successfully created user");
    Ok(HttpResponse::Ok().json(json!({ "message": format!("Account {} created", body.email)})))
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub struct CreateAccountBodyData {
    pub email: String,
    pub password: String,
}

#[derive(thiserror::Error)]
pub enum CreateAccountError {
    #[error("Invalid email: {0}")]
    InvalidEmail(String),
    #[error("Invalid password")]
    InvalidPassword(String),
    #[error("{0}")]
    UserInternalError(String),
    #[error(transparent)]
    RepositoryInternalError(#[from] UserPostgresRepositoryError),
    #[error(transparent)]
    InternalError(#[from] anyhow::Error),
}

impl std::fmt::Debug for CreateAccountError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        error_chain_fmt(self, f)
    }
}

impl ResponseError for CreateAccountError {
    fn status_code(&self) -> StatusCode {
        match self {
            CreateAccountError::InvalidEmail(_) | CreateAccountError::InvalidPassword(_) => {
                StatusCode::BAD_REQUEST
            }
            CreateAccountError::InternalError(_)
            | CreateAccountError::UserInternalError(_)
            | CreateAccountError::RepositoryInternalError(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    #[tracing::instrument(name = "Response error from create_account controller", skip(self), fields(error = %self))]
    fn error_response(&self) -> HttpResponse<actix_web::body::BoxBody> {
        HttpResponse::build(self.status_code())
            .insert_header(ContentType::json())
            .json(json!({ "error": self.to_string() }))
    }
}
