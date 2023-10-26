use async_trait::async_trait;
use common::helper::error_chain_fmt;
use meilisearch_sdk::{task_info::TaskInfo, Client};
use shaku::Component;
use tracing::info;

use crate::{domain::entities::content::ContentEntity, ports::content_repository::{ContentRepository, ContentRepositoryError}};

const DEFAULT_SEARCH_LIMIT: usize = 10;

/// Repository for `ContentEntity` persisted in Meilisearch
#[derive(Component)]
#[shaku(interface = ContentRepository)]
pub struct ContentMeilisearchRepository {
    client: Client,
    index: String,
}

impl ContentMeilisearchRepository {
    pub fn new(client: Client, index: String) -> Self {
        Self { client, index }
    }

    pub fn index(&self) -> String {
        self.index.clone()
    }
}
 
#[async_trait]
impl ContentRepository for ContentMeilisearchRepository {

    #[tracing::instrument(name = "Saving content to Meilishearch", skip(self))]
    async fn save(
        &self,
        content: &ContentEntity,
    ) -> Result<(), ContentRepositoryError> {
        // TODO: handle error
        let task: TaskInfo = self
            .client
            .index(&self.index)
            .add_or_replace(&[content], None)
            .await.unwrap();

        info!(?task, "Saved content");

        Ok(())
    }

    #[tracing::instrument(name = "Searching content from Meilishearch", skip(self))]
    async fn search(
        &self,
        query: &str,
        limit: Option<usize>,
    ) -> Result<
        // Vec<meilisearch_sdk::search::SearchResult<ContentEntity>>,
        (),
        ContentRepositoryError,
    > {
        let limit = limit.unwrap_or(DEFAULT_SEARCH_LIMIT);

        // TODO: handle error and result type
        let result = self
            .client
            .index(&self.index)
            .search()
            .with_query(query)
            .with_limit(limit)
            .execute::<ContentEntity>()
            .await.unwrap();

        info!(?result, "Result:");

        //Ok(result.hits)
        Ok(())
    }
}

#[derive(thiserror::Error)]
pub enum ContentMeilisearchRepositoryError {
    #[error(transparent)]
    MeilisearchError(#[from] meilisearch_sdk::errors::Error),
}

impl std::fmt::Debug for ContentMeilisearchRepositoryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        error_chain_fmt(self, f)
    }
}
