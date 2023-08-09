use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub type Embeddings = Vec<f32>;

#[derive(Debug, Deserialize, Serialize)]
pub struct ContentPoint {
    pub id: Uuid,
    pub payload: ContentPointPayload,
    pub vector: Embeddings,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ContentPointPayload {
    pub content: String,
    // TODO: enforces that extracted content metadata should have at least source_name and user_id
}
