use std::io::Read;

use serde_json::{json, Value as JsonValue};

use super::meta_read::MetaRead;

pub const SIMPLE_READER_META_KEY: &str = "simple";
pub const SIMPLE_READER_META_KEY_DEFAULT: &str = "default_key";

/// Helper struct to have a reader implementing MetaRead
///
/// Useful for tests.
pub struct SimpleMetadataReader<Reader: Read> {
    reader: Reader,
    metadata: JsonValue,
}

impl<Reader: Read> SimpleMetadataReader<Reader> {
    pub fn new(reader: Reader, metadata: Option<JsonValue>) -> Self {
        let metadata = metadata.unwrap_or(JsonValue::Null);
        let metadata = match metadata {
            JsonValue::Object(map) => json!(map),
            JsonValue::Null => JsonValue::Null,
            _ => json!({ SIMPLE_READER_META_KEY_DEFAULT: metadata }),
        };

        Self { reader, metadata }
    }
}

impl<Reader: Read> MetaRead for SimpleMetadataReader<Reader> {
    fn get_current_metadata(&self) -> JsonValue {
        json!({ SIMPLE_READER_META_KEY: self.metadata.clone() })
    }
}

impl<Reader: Read> Read for SimpleMetadataReader<Reader> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.reader.read(buf)
    }
}
