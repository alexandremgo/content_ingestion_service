use chrono::Utc;
use common::helper::error_chain_fmt;
use sqlx::PgExecutor;

use crate::domain::entities::source_meta::{SourceMeta, SourceType};

pub struct SourceMetaPostgresRepository {}

impl Default for SourceMetaPostgresRepository {
    fn default() -> Self {
        Self::new()
    }
}

impl SourceMetaPostgresRepository {
    pub fn new() -> Self {
        Self {}
    }

    #[tracing::instrument(name = "Saving new source meta in database", skip(self, db_executor))]
    pub async fn add_source_meta(
        &self,
        db_executor: impl PgExecutor<'_>,
        source_meta: &SourceMeta,
    ) -> Result<(), SourceMetaPostgresRepositoryError> {
        sqlx::query!(
            r#"
    INSERT INTO source_metas (id, user_id, object_store_name, source_type, initial_name, added_at, extracted_at)
    VALUES ($1, $2, $3, $4, $5, $6, NULL)
            "#,
            source_meta.id,
            source_meta.user_id,
            source_meta.object_store_name,
            source_meta.source_type.to_owned() as SourceType,
            source_meta.initial_name.to_string(),
            Utc::now()
        )
        .execute(db_executor)
        .await?;

        Ok(())
    }
}

#[derive(thiserror::Error)]
pub enum SourceMetaPostgresRepositoryError {
    #[error(transparent)]
    DBError(#[from] sqlx::Error),
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

impl std::fmt::Debug for SourceMetaPostgresRepositoryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        error_chain_fmt(self, f)
    }
}
