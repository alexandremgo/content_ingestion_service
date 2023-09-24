use actix_web::http::header::ContentType;
use actix_web::http::StatusCode;
use actix_web::{web, HttpResponse, ResponseError};
use common::helper::error_chain_fmt;
use secrecy::Secret;
use serde_json::json;
use sqlx::PgPool;
use tracing::{error, info};

use crate::domain::entities::user::UserError;
use crate::repositories::jwt_authentication_repository::{
    JwtAuthenticationRepository, JwtAuthenticationRepositoryError,
};
use crate::repositories::user_postgres_repository::UserPostgresRepository;
use crate::repositories::user_postgres_repository::UserPostgresRepositoryError;

/// Log in user account controller
///
/// Improvements:
/// - enforce almost constant time by using a default user if the email does not exist, in order to avoid email guessing via timing attacks
#[tracing::instrument(
    name = "Log in user account",
    skip(pool, user_repository, auth_repository, body)
)]
pub async fn log_in_account(
    pool: web::Data<PgPool>,
    user_repository: web::Data<UserPostgresRepository>,
    auth_repository: web::Data<JwtAuthenticationRepository>,
    body: web::Json<LogInAccountBodyData>,
) -> Result<HttpResponse, LogInAccountError> {
    let LogInAccountBodyData { email, password } = body.into_inner();
    let password = Secret::new(password);

    info!(email, "Login attempt");

    let stored_user =
        user_repository
            .check_user(&**pool, &email)
            .await
            .map_err(|error| match error {
                UserPostgresRepositoryError::UserDoesNotExist(error_message) => {
                    info!(
                        error = error_message,
                        email, "Attempt to login to non-existing user"
                    );
                    LogInAccountError::InvalidCredentials()
                }
                _ => error.into(),
            })?;

    let _result = stored_user
        .verify_password_hash(password)
        .await
        .map_err(|error| {
            error!(
                ?error,
                email, "Error when verifying password hash during login"
            );

            match error {
                UserError::InternalError(_) => LogInAccountError::InvalidCredentials(),
                _ => LogInAccountError::InternalError(error.into()),
            }
        })?;

    let jwt_token = auth_repository.create_token(&stored_user.id.to_string())?;

    Ok(HttpResponse::Ok().json(LogInAccountResponse {
        access_token: jwt_token,
        message: format!("Successfully logged in {}", email),
    }))
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub struct LogInAccountBodyData {
    pub email: String,
    pub password: String,
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub struct LogInAccountResponse {
    pub access_token: String,
    pub message: String,
}

#[derive(thiserror::Error)]
pub enum LogInAccountError {
    #[error(transparent)]
    RepositoryInternalError(#[from] UserPostgresRepositoryError),
    #[error(transparent)]
    InternalError(#[from] anyhow::Error),
    #[error("Invalid credentials")]
    InvalidCredentials(),
    #[error(transparent)]
    JwtAuthenticationRepositoryError(#[from] JwtAuthenticationRepositoryError),
}

impl std::fmt::Debug for LogInAccountError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        error_chain_fmt(self, f)
    }
}

impl ResponseError for LogInAccountError {
    fn status_code(&self) -> StatusCode {
        match self {
            LogInAccountError::InvalidCredentials() => StatusCode::UNAUTHORIZED,
            LogInAccountError::InternalError(_)
            | LogInAccountError::RepositoryInternalError(_)
            | LogInAccountError::JwtAuthenticationRepositoryError(_) => {
                StatusCode::INTERNAL_SERVER_ERROR
            }
        }
    }

    #[tracing::instrument(name = "Response error from create_account controller", skip(self), fields(error = %self))]
    fn error_response(&self) -> HttpResponse<actix_web::body::BoxBody> {
        HttpResponse::build(self.status_code())
            .insert_header(ContentType::json())
            .json(json!({ "error": self.to_string() }))
    }
}
