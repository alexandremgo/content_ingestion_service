use crate::domain::entities::user::{CheckingUser, CreatingUser};
use common::helper::error_chain_fmt;
use secrecy::Secret;
use sqlx::PgExecutor;

/// User repository implemented using Postgres
pub struct UserPostgresRepository {}

impl Default for UserPostgresRepository {
    fn default() -> Self {
        Self::new()
    }
}

impl UserPostgresRepository {
    pub fn new() -> Self {
        Self {}
    }

    #[tracing::instrument(name = "Saving new user in database", skip(self, db_executor))]
    pub async fn add_user(
        &self,
        db_executor: impl PgExecutor<'_>,
        user: &CreatingUser,
    ) -> Result<(), UserPostgresRepositoryError> {
        sqlx::query!(
            r#"
    INSERT INTO users (id, email, password_hash, created_at, updated_at)
    VALUES ($1, $2, $3, $4, $5)
            "#,
            user.id,
            user.email.as_ref(),
            user.password_hash.as_ref(),
            user.created_at,
            user.updated_at
        )
        .execute(db_executor)
        .await?;

        Ok(())
    }

    #[tracing::instrument(name = "Checking user in database", skip(self, db_executor))]
    pub async fn check_user(
        &self,
        db_executor: impl PgExecutor<'_>,
        email: &str,
    ) -> Result<CheckingUser, UserPostgresRepositoryError> {
        let record = sqlx::query!(
            r#"
    SELECT id, password_hash FROM users 
    WHERE email = $1
            "#,
            email,
        )
        .fetch_one(db_executor)
        .await
        .map_err(|_| UserPostgresRepositoryError::UserDoesNotExist(email.to_string()))?;

        let user = CheckingUser {
            id: record.id,
            password_hash: Secret::new(record.password_hash),
        };

        Ok(user)
    }
}

#[derive(thiserror::Error)]
pub enum UserPostgresRepositoryError {
    #[error(transparent)]
    DBError(#[from] sqlx::Error),
    #[error("{0}")]
    UserDoesNotExist(String),
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

impl std::fmt::Debug for UserPostgresRepositoryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        error_chain_fmt(self, f)
    }
}
