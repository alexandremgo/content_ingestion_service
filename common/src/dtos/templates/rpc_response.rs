use serde::{Deserialize, Serialize};

use crate::helper::error_chain_fmt;

#[derive(Debug, Deserialize, Serialize)]
pub enum RpcErrorStatus {
    BadRequest,
    InternalServerError,
    // Unauthorized,
}

#[derive(Debug, Deserialize, Serialize)]
pub enum RpcResponse<T> {
    Ok {
        data: T,
    },
    Error {
        status: RpcErrorStatus,
        message: String,
    },
}

impl<'a, T: Serialize + Deserialize<'a>> RpcResponse<T> {
    pub fn try_parsing(response: &'a [u8]) -> Result<Self, RpcResponseError> {
        let response = std::str::from_utf8(response)?;
        let response = serde_json::from_str(response)
            .map_err(|e| RpcResponseError::InvalidJsonData(e, response.to_string()))?;

        Ok(response)
    }

    pub fn try_serializing(response: &Self) -> Result<String, RpcResponseError> {
        let response = serde_json::to_string(response)
            .map_err(|e| RpcResponseError::InvalidResponse(e))?;

        Ok(response)
    }
}

#[derive(thiserror::Error)]
pub enum RpcResponseError {
    #[error("Data could not be converted from utf8 array to string")]
    InvalidUtf8Data(#[from] std::str::Utf8Error),

    #[error("Data did not represent a valid JSON RPC response: {0}. UTF-8 representation: {1}")]
    InvalidJsonData(serde_json::Error, String),

    #[error("Response could not be serialized from its JSON representation: {0}")]
    InvalidResponse(serde_json::Error),
}

impl std::fmt::Debug for RpcResponseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        error_chain_fmt(self, f)
    }
}
