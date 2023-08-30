use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;

use crate::helper::error_chain_fmt;

#[derive(Debug, Deserialize, Serialize)]
pub struct FulltextSearchRequestDto {
    pub metadata: JsonValue,
    pub query: String,
}

impl FulltextSearchRequestDto {
    pub fn try_parsing(data: &[u8]) -> Result<Self, FulltextSearchRequestDtoError> {
        let data = std::str::from_utf8(data)?;
        let my_data = serde_json::from_str(data)
            .map_err(|e| FulltextSearchRequestDtoError::InvalidJsonData(e, data.to_string()))?;

        Ok(my_data)
    }

    pub fn try_serializing(&self) -> Result<String, FulltextSearchRequestDtoError> {
        Ok(serde_json::to_string(self)
            .map_err(|e| FulltextSearchRequestDtoError::InvalidRequest(e))?)
    }
}

#[derive(thiserror::Error)]
pub enum FulltextSearchRequestDtoError {
    #[error("Data could not be converted from utf8 array to string")]
    InvalidUtf8Data(#[from] std::str::Utf8Error),
    #[error("Data did not represent a valid JSON object: {0}. Data: {1}")]
    InvalidJsonData(serde_json::Error, String),
    #[error("Request could not be serialized from its JSON representation: {0}")]
    InvalidRequest(serde_json::Error),
}

impl std::fmt::Debug for FulltextSearchRequestDtoError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        error_chain_fmt(self, f)
    }
}
