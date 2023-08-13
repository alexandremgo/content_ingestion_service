use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use uuid::Uuid;

#[derive(Debug, Deserialize, Serialize)]
pub struct ContentEntity {
    pub id: Uuid,
    pub metadata: JsonValue,
    pub content: String,
}
