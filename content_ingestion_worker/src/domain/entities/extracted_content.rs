use common::dtos::extracted_content::ExtractedContentDto;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use uuid::Uuid;

#[derive(Debug, Deserialize, Serialize)]
pub struct ExtractedContent {
    pub id: Uuid,
    pub metadata: JsonValue,
    pub content: String,
}

impl ExtractedContent {
    pub fn new(content: String, metadata: JsonValue) -> Self {
        Self {
            id: Uuid::new_v4(),
            metadata,
            content,
        }
    }
}

impl Into<ExtractedContentDto> for ExtractedContent {
    fn into(self) -> ExtractedContentDto {
        ExtractedContentDto {
            id: self.id,
            metadata: self.metadata,
            content: self.content,
        }
    }
}
