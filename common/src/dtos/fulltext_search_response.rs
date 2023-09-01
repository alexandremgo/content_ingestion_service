use super::templates::rpc_response::RpcResponse;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use uuid::Uuid;

#[derive(Debug, Deserialize, Serialize)]
pub struct ResultContent {
    pub id: Uuid,
    pub metadata: JsonValue,
    pub content: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct FulltextSearchResponseData {
    pub results: Vec<ResultContent>,
}

pub type FulltextSearchResponseDto = RpcResponse<FulltextSearchResponseData>;
