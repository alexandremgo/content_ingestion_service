use uuid::Uuid;

#[derive(Debug, serde::Serialize)]
pub struct ContentExtractJob {
    pub source_meta_id: Uuid,
}
