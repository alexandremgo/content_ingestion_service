use chrono::{DateTime, Utc};
use typed_builder::TypedBuilder;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub enum SourceType {
    Epub,
}

impl std::string::ToString for SourceType {
    fn to_string(&self) -> String {
        match self {
            SourceType::Epub => String::from("epub"),
        }
    }
}

impl std::str::FromStr for SourceType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "epub" => Ok(SourceType::Epub),
            _ => Err(format!("Invalid SourceType: {}", s)),
        }
    }
}

#[derive(Debug, Clone, TypedBuilder)]
pub struct SourceMeta {
    #[builder(default=Uuid::new_v4())]
    pub id: Uuid,

    pub user_id: Uuid,

    /// File name received from the user
    pub initial_name: String,

    /// Name of the file saved in the object store
    pub object_store_name: String,

    pub source_type: SourceType,

    #[builder(default=Utc::now())]
    pub added_at: DateTime<Utc>,

    #[builder(default)]
    pub extracted_at: Option<DateTime<Utc>>,
}
