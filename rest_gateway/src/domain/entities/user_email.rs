use common::helper::error_chain_fmt;
use validator::validate_email;

#[derive(Debug, Clone)]
pub struct UserEmail(String);

impl UserEmail {
    pub fn parse(s: &str) -> Result<UserEmail, UserEmailError> {
        if validate_email(s) {
            Ok(Self(s.to_string()))
        } else {
            Err(UserEmailError::InvalidEmailFormat(format!(
                "{} is not a valid user email.",
                s
            )))
        }
    }
}

impl AsRef<str> for UserEmail {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for UserEmail {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(thiserror::Error)]
pub enum UserEmailError {
    #[error("Invalid email format: {0}")]
    InvalidEmailFormat(String),
}

impl std::fmt::Debug for UserEmailError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        error_chain_fmt(self, f)
    }
}

#[cfg(test)]
mod tests {
    use super::UserEmail;
    use claims::assert_err;
    use fake::faker::internet::en::SafeEmail;
    use fake::Fake;
    use quickcheck::Gen;
    use rand::rngs::StdRng;
    use rand::SeedableRng;

    #[derive(Debug, Clone)]
    struct ValidEmailFixture(pub String);

    // Randomly generates a valid email
    impl quickcheck::Arbitrary for ValidEmailFixture {
        fn arbitrary(g: &mut Gen) -> Self {
            let mut rng = StdRng::seed_from_u64(u64::arbitrary(g));
            let email = SafeEmail().fake_with_rng(&mut rng);
            Self(email)
        }
    }

    #[quickcheck_macros::quickcheck]
    fn valid_emails_are_parsed_successfully(valid_email: ValidEmailFixture) -> bool {
        UserEmail::parse(&valid_email.0).is_ok()
    }

    #[test]
    fn empty_string_is_rejected() {
        let email = "";
        assert_err!(UserEmail::parse(email));
    }

    #[test]
    fn email_missing_at_symbol_is_rejected() {
        let email = "ursuladomain.com";
        assert_err!(UserEmail::parse(email));
    }

    #[test]
    fn email_missing_subject_is_rejected() {
        let email = "@domain.com";
        assert_err!(UserEmail::parse(email));
    }
}
