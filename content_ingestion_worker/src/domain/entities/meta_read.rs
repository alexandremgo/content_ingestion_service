use serde_json::Value as JsonValue;

/// Metadata about the current read
pub trait MetaRead {
    fn get_current_metadata(&self) -> JsonValue;
}
