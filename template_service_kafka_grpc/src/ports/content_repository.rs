use async_trait::async_trait;
use common::helper::error_chain_fmt;
use shaku::Interface;

use crate::domain::entities::content::ContentEntity;

#[async_trait]
pub trait ContentRepository: Interface {
    async fn save(
        &self,
        content: &ContentEntity,
    ) -> Result<(), ContentRepositoryError>;

    async fn search(
        &self,
        query: &str,
        limit: Option<usize>,
    ) -> Result<
        // Vec<meilisearch_sdk::search::SearchResult<ContentEntity>>,
        (),
        ContentRepositoryError,
    >; 
}

#[derive(thiserror::Error)]
pub enum ContentRepositoryError {}

impl std::fmt::Debug for ContentRepositoryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        error_chain_fmt(self, f)
    }
}
