use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;

#[derive(Debug, Deserialize, Serialize)]
pub struct ExtractedContent {
    pub metadata: JsonValue,
    pub content: String,
}
