use serde::{Deserialize, Serialize};
use super::templates::rpc_response::RpcResponse;

#[derive(Debug, Deserialize, Serialize)]
pub struct FulltextSearchResponseData {
    pub content: String,
}

pub type FulltextSearchResponseDto = RpcResponse<FulltextSearchResponseData>;
