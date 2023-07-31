use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::helper::error_chain_fmt;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SourceType {
    Epub,
}

/// Represents a request for a job to extract content from a source file
#[derive(Debug, Serialize, Deserialize)]
pub struct ExtractContentJob {
    /// Id of the source meta associated to the file the job is working on
    pub source_meta_id: Uuid,

    /// Path and name of the file saved in the object store
    pub object_store_path_name: String,

    /// Type of source file
    pub source_type: SourceType,

    /// Initial name of the source
    pub source_initial_name: String,
}

impl ExtractContentJob {
    pub fn try_parsing(data: &Vec<u8>) -> Result<Self, ExtractContentJobParsingError> {
        let data = std::str::from_utf8(data)?;
        let my_data = serde_json::from_str(data)
            .map_err(|e| ExtractContentJobParsingError::InvalidJsonData(e, data.to_string()))?;

        Ok(my_data)
    }
}

#[derive(thiserror::Error)]
pub enum ExtractContentJobParsingError {
    #[error("Data could not be converted from utf8 u8 vector to string")]
    InvalidStringData(#[from] std::str::Utf8Error),

    #[error("Data did not represent a valid JSON object: {0}. Data: {1}")]
    InvalidJsonData(serde_json::Error, String),
}

impl std::fmt::Debug for ExtractContentJobParsingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        error_chain_fmt(self, f)
    }
}
