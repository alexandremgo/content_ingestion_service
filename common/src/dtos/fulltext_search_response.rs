use super::templates::rpc_response::RpcResponse;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize)]
pub struct FulltextSearchResponseData {
    pub content: String,
}

pub type FulltextSearchResponseDto = RpcResponse<FulltextSearchResponseData>;
