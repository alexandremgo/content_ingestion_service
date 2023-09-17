use argon2::{
    password_hash::SaltString, Algorithm, Argon2, Params, PasswordHash, PasswordHasher,
    PasswordVerifier, Version,
};
use common::helper::error_chain_fmt;
use secrecy::{ExposeSecret, Secret};

/// PHC format hashed password
///
/// PHC format: the salt and information about the hash function is stored for each user
/// This makes it possible to update our hash function and still be backwards compatible.
/// Also, persisting the salt with the password is preventing a pre-compiled dictionary attack
/// (the attacker would need to re-compute their dictionary for each user)
#[derive(Debug, Clone)]
pub struct UserPassword(Secret<String>);

impl UserPassword {
    /// Computes a PHC-format password
    ///
    /// CPU-intensive task: it is a good idea to run it in another thread
    #[tracing::instrument(name = "Computing password in PHC format", skip(password))]
    pub fn compute_password_hash(
        password: Secret<String>,
    ) -> Result<UserPassword, UserPasswordError> {
        let salt = SaltString::generate(&mut rand::thread_rng());

        let password_hash = Argon2::new(
            Algorithm::Argon2id,
            Version::V0x13,
            Params::new(15000, 2, 1, None).unwrap(),
        )
        .hash_password(password.expose_secret().as_bytes(), &salt)?
        .to_string();

        Ok(UserPassword(Secret::new(password_hash)))
    }

    /// Parses a PHC-format hashed password
    ///
    /// Parsing it to check if there is an error,
    /// but saving it as a String in our UserPassword struct to avoid coloring our code with lifetimes
    ///
    /// # Params
    /// - `password_hash_str`: serialized PHC-format hashed password
    #[tracing::instrument(name = "Parsing password hash", skip(password_hash_str))]
    pub fn parse(password_hash_str: Secret<String>) -> Result<UserPassword, UserPasswordError> {
        let expected_password_hash = PasswordHash::new(password_hash_str.expose_secret())?;
        Ok(UserPassword(Secret::new(
            expected_password_hash.serialize().to_string(),
        )))
    }

    /// Verifies a password against a PHC-format hashed password
    ///
    /// CPU-intensive task: it is a good idea to run it in another thread
    ///
    /// # Params
    /// - `expected_password_hash`: The PHC-format hashed password
    /// - `password_candidate`: The password to verify
    #[tracing::instrument(name = "Verifying password hash", skip(self, password_candidate))]
    pub fn verify(&self, password_candidate: Secret<String>) -> Result<(), UserPasswordError> {
        let expected_password_hash = PasswordHash::new(self.0.expose_secret())?;

        Argon2::default()
            .verify_password(
                password_candidate.expose_secret().as_bytes(),
                &expected_password_hash,
            )
            .map_err(|e| {
                UserPasswordError::InvalidCredentials(format!("Invalid password: {:?}", e))
            })
    }
}

impl AsRef<str> for UserPassword {
    fn as_ref(&self) -> &str {
        &self.0.expose_secret()
    }
}

#[derive(thiserror::Error)]
pub enum UserPasswordError {
    #[error(transparent)]
    HashError(#[from] argon2::password_hash::Error),
    #[error("Invalid crendentials: {0}")]
    InvalidCredentials(String),
}

impl std::fmt::Debug for UserPasswordError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        error_chain_fmt(self, f)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    // use claims::assert_err;
    use fake::faker::internet::en::Password;
    use fake::Fake;
    use secrecy::Secret;

    #[test]
    fn a_password_can_be_hashed_and_verified() {
        let password = Password(8..24).fake();
        let password = Secret::new(password);

        let password_hash = UserPassword::compute_password_hash(password.clone()).unwrap();
        let check = password_hash.verify(password);

        assert!(check.is_ok())
    }

    #[test]
    fn a_password_hashed_with_other_algo_can_be_verified() {
        let password: String = Password(8..24).fake();
        let salt = SaltString::generate(&mut rand::thread_rng());

        // Changing the params compared to the one used in `compute_password_hash`
        let password_hash = Argon2::new(
            Algorithm::Argon2id,
            Version::V0x13,
            Params::new(4242, 4, 1, Some(16)).unwrap(),
        )
        .hash_password(password.as_bytes(), &salt)
        .unwrap()
        .to_string();

        let password_hash = UserPassword::parse(Secret::new(password_hash)).unwrap();

        let check = password_hash.verify(Secret::new(password));

        assert!(check.is_ok())
    }
}
