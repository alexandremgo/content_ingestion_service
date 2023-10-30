use chrono::{DateTime, Utc};
use common::dtos::extract_content_job::SourceTypeDto;
use std::str::FromStr;
use typed_builder::TypedBuilder;
use uuid::Uuid;

#[derive(Debug, Clone, sqlx::Type, serde::Serialize)]
#[sqlx(type_name = "source_type", rename_all = "lowercase")]
pub enum SourceType {
    Epub,
}

impl FromStr for SourceType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "epub" => Ok(SourceType::Epub),
            _ => Err(format!("Invalid SourceType: {}", s)),
        }
    }
}

impl From<SourceType> for SourceTypeDto {
    fn from(value: SourceType) -> Self {
        match value {
            SourceType::Epub => SourceTypeDto::Epub,
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
