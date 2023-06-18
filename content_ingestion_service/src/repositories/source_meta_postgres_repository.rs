use chrono::Utc;
use sqlx::{Postgres, Transaction};

use crate::{
    domain::entities::source_meta::{SourceMeta, SourceType},
    helper::error_chain_fmt,
};

pub struct SourceMetaPostgresRepository {
    // Not needed as a transaction is always used
    // pg_pool: PgPool,
}

impl SourceMetaPostgresRepository {
    pub fn new() -> Self {
        Self {}
    }

    #[tracing::instrument(name = "Saving new source meta in database", skip(self, transaction))]
    pub async fn add_source_meta(
        &self,
        transaction: &mut Transaction<'_, Postgres>,
        source_meta: &SourceMeta,
    ) -> Result<(), SourceMetaPostgresRepositoryError> {
        sqlx::query!(
            r#"
    INSERT INTO source_meta (id, user_id, object_store_name, source_type, initial_name, added_at, extracted_at)
    VALUES ($1, $2, $3, $4, $5, $6, NULL)
            "#,
            source_meta.id,
            source_meta.user_id,
            source_meta.object_store_name,
            source_meta.source_type.to_owned() as SourceType,
            source_meta.initial_name.to_string(),
            Utc::now()
        )
        .execute(transaction)
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
