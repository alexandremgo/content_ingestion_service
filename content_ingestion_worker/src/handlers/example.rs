use crate::helper::error_chain_fmt;
use tracing::info;

#[derive(serde::Serialize, serde::Deserialize, Debug)]
pub struct MyData {
    pub field_1: String,
    pub field_2: String,
}

#[derive(thiserror::Error)]
pub enum MyDataParsingError {
    #[error("Data could not be converted from utf8 u8 vector to string")]
    InvalidStringData(#[from] std::str::Utf8Error),

    #[error("Data did not represent a valid JSON object: {0}")]
    InvalidJsonData(#[from] serde_json::Error),
}

impl std::fmt::Debug for MyDataParsingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        error_chain_fmt(self, f)
    }
}

impl MyData {
    pub fn try_parsing(data: &Vec<u8>) -> Result<Self, MyDataParsingError> {
        let data = std::str::from_utf8(data)?;
        let my_data = serde_json::from_str(data)?;

        Ok(my_data)
    }
}

#[tracing::instrument(name = "Handling queued job")]
pub fn handler(my_data: MyData) -> Result<(), String> {
    // Do something with the delivery data (The message payload)
    info!("🍕 Received data: {:?}\n", my_data);

    Ok(())
}
