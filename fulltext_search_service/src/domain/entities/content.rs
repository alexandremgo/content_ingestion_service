use common::dtos::extracted_content::ExtractedContentDto;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use uuid::Uuid;

#[derive(Debug, Deserialize, Serialize)]
pub struct ContentEntity {
    pub id: Uuid,
    pub metadata: JsonValue,
    pub content: String,
}

impl From<ExtractedContentDto> for ContentEntity {
    fn from(value: ExtractedContentDto) -> Self {
        Self {
            id: value.id,
            metadata: value.metadata,
            content: value.content,
        }
    }
} 
