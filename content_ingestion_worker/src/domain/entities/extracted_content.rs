use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use uuid::Uuid;

#[derive(Debug, Deserialize, Serialize)]
pub struct ExtractedContent {
    pub id: String,
    pub metadata: JsonValue,
    pub content: String,
}

impl ExtractedContent {
    pub fn new(content: String, metadata: JsonValue) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            metadata,
            content,
        }
    }
}
