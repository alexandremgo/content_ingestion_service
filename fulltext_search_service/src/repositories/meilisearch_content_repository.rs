use common::helper::error_chain_fmt;
use meilisearch_sdk::{task_info::TaskInfo, Client};
use tracing::info;

use crate::domain::entities::content::ContentEntity;

/// Repository for `ContentEntity` persisted in Meilisearch
pub struct MeilisearchContentRepository {
    client: Client,
    index: String,
}

impl MeilisearchContentRepository {
    pub fn new(client: Client, index: String) -> Self {
        Self { client, index }
    }

    #[tracing::instrument(name = "Saving content to Meilishearch", skip(self))]
    pub async fn save(
        &self,
        content: &ContentEntity,
    ) -> Result<(), MeilisearchContentRepositoryError> {
        let task: TaskInfo = self
            .client
            .index(&self.index)
            .add_or_replace(&[content], None)
            .await?;

        info!(?task, "Saved content");

        Ok(())
    }

    #[tracing::instrument(name = "Searching content from Meilishearch", skip(self))]
    pub async fn search(
        &self,
        query: &str,
    ) -> Result<
        Vec<meilisearch_sdk::search::SearchResult<ContentEntity>>,
        MeilisearchContentRepositoryError,
    > {
        let result = self
            .client
            .index(&self.index)
            .search()
            .with_query(query)
            .execute::<ContentEntity>()
            .await?;

        info!(?result, "Result:");

        Ok(result.hits)
    }
}

#[derive(thiserror::Error)]
pub enum MeilisearchContentRepositoryError {
    #[error(transparent)]
    MeilisearchError(#[from] meilisearch_sdk::errors::Error),
}

impl std::fmt::Debug for MeilisearchContentRepositoryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        error_chain_fmt(self, f)
    }
}
