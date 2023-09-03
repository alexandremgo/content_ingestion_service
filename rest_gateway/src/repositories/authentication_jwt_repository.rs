use actix_web::http::StatusCode;
use actix_web::ResponseError;
use chrono::{Duration, Utc};
use common::helper::error_chain_fmt;
use jsonwebtoken::{decode, encode, Algorithm, DecodingKey, EncodingKey, Header, Validation};
use secrecy::{ExposeSecret, Secret};
use serde::{Deserialize, Serialize};

/// Repository to handle JWT tokens
///
/// Used in middleware and in route handler.
#[derive(Clone)]
pub struct AuthenticationJwtRepository {
    secret: Secret<String>,
    expire_in_s: i64,
}

// TODO: iss: issuer in claims ?
#[derive(Debug, Serialize, Deserialize)]
pub struct TokenClaims {
    /// Subject
    pub sub: String,

    /// Issued At
    pub iat: usize,

    /// Expires At
    pub exp: usize,
}

impl AuthenticationJwtRepository {
    pub fn new(secret: Secret<String>, expire_in_s: i64) -> Self {
        Self {
            secret,
            expire_in_s,
        }
    }

    /// Creates a new JWT token
    #[tracing::instrument(name = "Create JWT token", skip(self))]
    pub fn create_token(&self, user_id: &str) -> Result<String, AuthenticationJwtRepositoryError> {
        if user_id.is_empty() {
            return Err(AuthenticationJwtRepositoryError::InvalidData(
                "Missing user id".to_string(),
            ));
        }

        let now = Utc::now();
        let iat = now.timestamp() as usize;
        let exp = (now + Duration::seconds(self.expire_in_s)).timestamp() as usize;
        let claims: TokenClaims = TokenClaims {
            sub: user_id.to_string(),
            exp,
            iat,
        };

        encode(
            &Header::default(),
            &claims,
            &EncodingKey::from_secret(self.secret.expose_secret().as_bytes()),
        )
        .map_err(|err| AuthenticationJwtRepositoryError::EncodingError(err))
    }

    /// Decodes a JWT token
    #[tracing::instrument(name = "Decode JWT token", skip(self))]
    pub fn decode_token(&self, token: &str) -> Result<String, AuthenticationJwtRepositoryError> {
        let decoded = decode::<TokenClaims>(
            token.into(),
            &DecodingKey::from_secret(self.secret.expose_secret().as_bytes()),
            &Validation::new(Algorithm::HS256),
        );

        // TODO: need to check expiring time, or `decode` handles it ?
        match decoded {
            Ok(token) => Ok(token.claims.sub),
            Err(err) => Err(AuthenticationJwtRepositoryError::DecodingError(err)),
        }
    }
}

#[derive(thiserror::Error)]
pub enum AuthenticationJwtRepositoryError {
    #[error("Invalid JWT token while decoding: {0}")]
    DecodingError(jsonwebtoken::errors::Error),
    #[error("Error while encoding JWT token: {0}")]
    EncodingError(jsonwebtoken::errors::Error),
    #[error("Invalid data to create JWT token: {0}")]
    InvalidData(String),
}

impl std::fmt::Debug for AuthenticationJwtRepositoryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        error_chain_fmt(self, f)
    }
}

impl ResponseError for AuthenticationJwtRepositoryError {
    fn status_code(&self) -> StatusCode {
        match self {
            AuthenticationJwtRepositoryError::InvalidData(_)
            | AuthenticationJwtRepositoryError::EncodingError(_) => {
                StatusCode::INTERNAL_SERVER_ERROR
            }
            AuthenticationJwtRepositoryError::DecodingError(_) => StatusCode::UNAUTHORIZED,
        }
    }
}

#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn on_valid_token_it_should_create_and_decode_correctly() {
        let user_id = "user123";
        let secret = Secret::new("my-secret-key".to_string());
        let auth_repo = AuthenticationJwtRepository::new(secret, 60);

        let token = auth_repo.create_token(user_id).unwrap();
        let decoded_user_id = auth_repo.decode_token(&token).unwrap();

        assert_eq!(decoded_user_id, user_id);
    }

    #[test]
    fn on_empty_user_id_token_create_should_fail() {
        let user_id = "";
        let secret = Secret::new("my-secret-key".to_string());
        let auth_repo = AuthenticationJwtRepository::new(secret, 60);

        let result = auth_repo.create_token(user_id);

        assert!(result.is_err());
        assert!(matches!(
            result,
            Err(AuthenticationJwtRepositoryError::InvalidData(_))
        ))
    }

    #[test]
    fn on_invalid_token_decode_should_fail() {
        let secret = Secret::new("my-secret-key".to_string());
        let invalid_token = "invalid-token";
        let auth_repo = AuthenticationJwtRepository::new(secret, 60);

        let result = auth_repo.decode_token(invalid_token);

        assert!(result.is_err());

        assert!(matches!(
            result,
            Err(AuthenticationJwtRepositoryError::DecodingError(_))
        ));
    }

    #[test]
    fn on_expired_token_decode_should_fail() {
        let secret = Secret::new("my-secret-key".to_string());
        let auth_repo = AuthenticationJwtRepository::new(secret, -60);

        let expired_token = auth_repo.create_token("user123").unwrap();
        let result = auth_repo.decode_token(&expired_token);

        println!("RESULT: {:?}", result);

        assert!(result.is_err());
        assert!(matches!(
            result,
            Err(AuthenticationJwtRepositoryError::DecodingError(_))
        ));
    }
}
