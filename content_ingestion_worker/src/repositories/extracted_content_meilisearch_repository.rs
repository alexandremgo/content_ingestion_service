use meilisearch_sdk::{task_info::TaskInfo, Client};
use tracing::info;

use crate::{domain::entities::extracted_content::ExtractedContent, helper::error_chain_fmt};

/// Repository for `ExtractedContent` persisted in Meilisearch
pub struct ExtractedContentMeilisearchRepository {
    client: Client,
    index: String,
}

impl ExtractedContentMeilisearchRepository {
    pub fn new(client: Client, index: String) -> Self {
        Self { client, index }
    }

    #[tracing::instrument(name = "Saving new extracted content to Meilishearch", skip(self))]
    pub async fn save(
        &self,
        extracted_content: &ExtractedContent,
    ) -> Result<(), ExtractedContentMeilisearchRepositoryError> {
        let task: TaskInfo = self
            .client
            .index(&self.index)
            .add_or_replace(&[extracted_content], None)
            .await?;

        info!("🍕 task: {:?}", task);

        Ok(())
    }
}

#[derive(thiserror::Error)]
pub enum ExtractedContentMeilisearchRepositoryError {
    #[error(transparent)]
    MeilisearchError(#[from] meilisearch_sdk::errors::Error),
}

impl std::fmt::Debug for ExtractedContentMeilisearchRepositoryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        error_chain_fmt(self, f)
    }
}
