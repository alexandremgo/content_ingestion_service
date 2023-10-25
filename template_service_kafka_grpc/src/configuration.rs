use secrecy::Secret;
use serde::Deserialize;
use serde_aux::field_attributes::deserialize_number_from_string;

#[derive(Debug, Deserialize, Clone)]
pub struct Settings {
    pub application: ApplicationSettings,
    pub kafka: KafkaSettings,
    pub grpc: GrpcSettings,
    pub meilisearch: MeilisearchSettings,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ApplicationSettings {
    pub host: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct KafkaSettings {
    pub group_id_prefix: String,

    #[serde(deserialize_with = "deserialize_number_from_string")]
    pub bootstrap_port: u16,
    pub bootstrap_host: String,
}


#[derive(Debug, Deserialize, Clone)]
pub struct GrpcSettings {
    pub app_server_port: u16,
}

#[derive(Debug, Deserialize, Clone)]
pub struct MeilisearchSettings {
    pub api_key: Secret<String>,
    #[serde(deserialize_with = "deserialize_number_from_string")]
    pub port: u16,
    pub host: String,
    pub contents_index: String,
}

impl MeilisearchSettings {
    pub fn endpoint(&self) -> String {
        format!("http://{}:{}", self.host, self.port)
    }
}


/// Extracts app settings from configuration files and env variables
///
/// `base.yml` should contain shared settings for all environments.
/// A specific env file should be created for each environment: `develop.yml`,`local.yml` and `production.yml`
/// The environment is set with the env var `APP_ENVIRONMENT`.
/// If `APP_ENVIRONMENT` is not set, `develop.yml` is the default.
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
    let environment_filename = format!("{}.yml", environment.as_str());

    let settings = config::Config::builder()
        .add_source(config::File::from(configuration_directory.join("base.yml")))
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
