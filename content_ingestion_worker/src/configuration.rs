use lapin::ConnectionProperties;
use secrecy::Secret;
use serde::Deserialize;
use serde_aux::field_attributes::deserialize_number_from_string;

#[derive(Debug, Deserialize, Clone)]
pub struct Settings {
    pub application: ApplicationSettings,
    pub object_storage: ObjectStorageSettings,
    pub rabbitmq: RabbitMQSettings,
    pub meilisearch: MeilisearchSettings,
}

// TODO: is it used for our worker ?
#[derive(Debug, Deserialize, Clone)]
pub struct ApplicationSettings {
    #[serde(deserialize_with = "deserialize_number_from_string")]
    pub port: u16,
    pub host: String,
}

#[derive(Deserialize, Debug, Clone)]
pub struct ObjectStorageSettings {
    pub username: String,
    pub password: Secret<String>,
    #[serde(deserialize_with = "deserialize_number_from_string")]
    pub port: u16,
    pub host: String,
    pub region: String,
    /// A bucket for each environment
    pub bucket_name: String,
}

impl ObjectStorageSettings {
    pub fn endpoint(&self) -> String {
        format!("http://{}:{}", self.host, self.port)
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct RabbitMQSettings {
    // pub username: String,
    // pub password: Secret<String>,
    #[serde(deserialize_with = "deserialize_number_from_string")]
    pub port: u16,
    pub host: String,
    /// Useful to create parallel queues during tests for example.
    pub queue_name_prefix: String,
}

impl RabbitMQSettings {
    pub fn get_uri(&self) -> String {
        format!("amqp://{}:{}", &self.host, &self.port)
    }

    pub fn get_connection_properties(&self) -> ConnectionProperties {
        ConnectionProperties::default()
            // Use tokio executor and reactor.
            // At the moment the reactor is only available for unix.
            .with_executor(tokio_executor_trait::Tokio::current())
            .with_reactor(tokio_reactor_trait::Tokio)
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct MeilisearchSettings {
    pub api_key: Secret<String>,
    #[serde(deserialize_with = "deserialize_number_from_string")]
    pub port: u16,
    pub host: String,
    pub extracted_content_index: String,
}

impl MeilisearchSettings {
    pub fn endpoint(&self) -> String {
        format!("http://{}:{}", self.host, self.port)
    }
}

/// Extracts app settings from configuration files and env variables
///
/// `base.yaml` should contain shared settings for all environments.
/// A specific env file should be created for each environment: `develop.yaml`,`local.yaml` and `production.yaml`
/// The environment is set with the env var `APP_ENVIRONMENT`.
/// If `APP_ENVIRONMENT` is not set, `develop.yaml` is the default.
///
/// Settings are also taken from environment variables: with a prefix of APP and '__' as separator
/// For ex: `APP_APPLICATION__PORT=5001 would set `Settings.application.port`
pub fn get_configuration() -> Result<Settings, config::ConfigError> {
    let base_path = std::env::current_dir().expect("Failed to determine the current directory");
    let configuration_directory = base_path.join("configuration");

    // Detects the running environment.
    // Default to `develop` if unspecified.
    let environment: Environment = std::env::var("APP_ENVIRONMENT")
        .unwrap_or_else(|_| "develop".into())
        .try_into()
        .expect("Failed to parse APP_ENVIRONMENT.");
    let environment_filename = format!("{}.yaml", environment.as_str());

    let settings = config::Config::builder()
        .add_source(config::File::from(
            configuration_directory.join("base.yaml"),
        ))
        .add_source(config::File::from(
            configuration_directory.join(environment_filename),
        ))
        // Adds in settings from environment variables (with a prefix of APP and '__' as separator)
        .add_source(
            config::Environment::with_prefix("APP")
                .prefix_separator("_")
                .separator("__"),
        )
        .build()?;

    settings.try_deserialize::<Settings>()
}

/// The possible runtime environment for our application.
pub enum Environment {
    Develop,
    Local,
    Production,
}

impl Environment {
    pub fn as_str(&self) -> &'static str {
        match self {
            Environment::Develop => "develop",
            Environment::Local => "local",
            Environment::Production => "production",
        }
    }
}

impl TryFrom<String> for Environment {
    type Error = String;

    fn try_from(s: String) -> Result<Self, Self::Error> {
        match s.to_lowercase().as_str() {
            "develop" => Ok(Self::Develop),
            "local" => Ok(Self::Local),
            "production" => Ok(Self::Production),
            other => Err(format!(
                "{} is not a supported environment. Use either `develop`, `local` or `production`.",
                other
            )),
        }
    }
}
