use chrono::{DateTime, Utc};
use common::{helper::error_chain_fmt, telemetry::spawn_blocking_with_tracing};
use secrecy::Secret;
use tracing::info;
use uuid::Uuid;

use super::{
    user_email::{UserEmail, UserEmailError},
    user_password::{UserPassword, UserPasswordError},
};

/// Represents a user.
///
/// The user entity is used in different contexts:
/// while creating it, when checking if the user exists etc.
///
/// `email` and `password` are wrapped in a `Secret` to avoid leaks in logs.
#[derive(Debug, Clone)]
pub enum User {
    Creating(CreatingUser),
    Checking(CheckingUser),
}

#[derive(Debug, Clone)]
pub struct CreatingUser {
    pub id: Uuid,
    pub email: UserEmail,
    pub password_hash: UserPassword,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct CheckingUser {
    pub id: Uuid,
    pub password_hash: Secret<String>,
}

impl User {
    /// Creates a user, generating their hashed password
    ///
    /// CPU-intensive task: better to run in a another thread using for example tokio::task::spawn_blocking
    pub async fn create(email: &str, password: Secret<String>) -> Result<CreatingUser, UserError> {
        let email = UserEmail::parse(email)?;
        info!(email = email.as_ref(), "Valid email");

        let password_hash =
            spawn_blocking_with_tracing(move || UserPassword::compute_password_hash(password))
                .await
                .map_err(|e| {
                    UserError::InternalError(format!(
                        "Unexpected error when spawning blocking thread: {}",
                        e
                    ))
                })??;

        Ok(CreatingUser {
            id: Uuid::new_v4(),
            email,
            password_hash,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        })
    }
}

impl CheckingUser {
    /// Verifies a password against a PHC-format hashed password
    ///
    /// The (CPU-intensive) task is run in another thread
    ///
    /// # Params
    /// - `password_candidate`: The password to verify
    #[tracing::instrument(name = "Verifying user password hash", skip(self, password_candidate))]
    pub async fn verify_password_hash(
        &self,
        password_candidate: Secret<String>,
    ) -> Result<(), UserError> {
        let password_hash = UserPassword::parse(&self.password_hash)?;

        return spawn_blocking_with_tracing(move || password_hash.verify(password_candidate))
            .await
            .map_err(|e| {
                UserError::InternalError(format!(
                    "Unexpected error when spawning blocking thread: {}",
                    e
                ))
            })?
            .map_err(|e| match e {
                UserPasswordError::InvalidCredentials(message) => {
                    UserError::InvalidCredentials(message)
                }
                _ => UserError::PasswordError(e),
            });
    }
}

#[derive(thiserror::Error)]
pub enum UserError {
    #[error(transparent)]
    PasswordError(#[from] UserPasswordError),
    #[error("{0}")]
    InvalidCredentials(String),
    #[error(transparent)]
    EmailError(#[from] UserEmailError),
    #[error("Internal: {0}")]
    InternalError(String),
}

impl std::fmt::Debug for UserError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        error_chain_fmt(self, f)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    // use claims::assert_err;
    use fake::faker::internet::en::{Password, SafeEmail};
    use fake::Fake;
    use secrecy::Secret;

    #[tokio::test]
    async fn valid_info_should_create_a_user() {
        // Arranges
        let password = Password(8..24).fake();
        let password = Secret::new(password);
        let email: String = SafeEmail().fake();

        // Acts
        let user = User::create(&email, password).await;

        // Asserts
        assert!(user.is_ok());
    }

    #[tokio::test]
    async fn a_valid_password_should_verify_correctly_user_password_hash() {
        // Arranges
        let password = Password(8..24).fake();
        let password = Secret::new(password);
        let email: String = SafeEmail().fake();
        let created_user = User::create(&email, password.clone()).await.unwrap();

        let checking_user = CheckingUser {
            id: created_user.id,
            password_hash: Secret::new(created_user.password_hash.as_ref().to_string()),
        };

        // Acts
        let result = checking_user.verify_password_hash(password).await;

        // Asserts
        assert!(result.is_ok());
    }
}
