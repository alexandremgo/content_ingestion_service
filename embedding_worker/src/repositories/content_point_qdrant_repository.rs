use std::collections::HashMap;

use common::helper::error_chain_fmt;
use qdrant_client::{
    prelude::QdrantClient,
    qdrant::{
        self, vectors_config::Config, CreateCollection, Distance, PointStruct, VectorParams,
        VectorsConfig,
    },
};
use tracing::info;

use crate::domain::entities::content_point::{ContentPoint, ContentPointPayload};

/// Repository for (extracted) content vectors (ContentVector) persisted in Qdrant
pub struct ContentPointQdrantRepository {
    client: QdrantClient,
    collection_name: String,
}

impl ContentPointQdrantRepository {
    #[tracing::instrument(
        name = "Initializing Qdrant and the associated collection",
        skip(client)
    )]
    pub async fn try_new(
        client: QdrantClient,
        collection_name: &str,
        collection_distance: &str,
        collection_vector_size: u64,
    ) -> Result<Self, ContentPointQdrantRepositoryError> {
        let collection_distance = Distance::from_str_name(&collection_distance).ok_or(
            ContentPointQdrantRepositoryError::QdrantConfigurationError(
                "Invalid Qdrant distance from configuration, using Dot distance".into(),
            ),
        )?;

        // Not idempotent
        // TODO: use collection_distance
        match client
            .create_collection(&CreateCollection {
                collection_name: collection_name.to_string(),
                vectors_config: Some(VectorsConfig {
                    config: Some(Config::Params(VectorParams {
                        size: collection_vector_size,
                        distance: collection_distance as i32,
                        ..Default::default()
                    })),
                }),
                ..Default::default()
            })
            .await
        {
            Ok(_) => (),
            Err(error) => {
                // Qdrant client only returns anyhow errors for now
                if !error.to_string().contains("already exists") {
                    info!(?error, "Error on config");
                    return Err(ContentPointQdrantRepositoryError::QdrantError(
                        error.to_string(),
                    ));
                }
            }
        };

        Ok(Self {
            client,
            collection_name: collection_name.to_string(),
        })
    }

    #[tracing::instrument(name = "Saving content points to Qdrant", skip(self))]
    pub async fn batch_save(
        &self,
        content_points: Vec<ContentPoint>,
    ) -> Result<(), ContentPointQdrantRepositoryError> {
        self.client
            .upsert_points(
                &self.collection_name,
                content_points.into_iter().map(PointStruct::from).collect(),
                None,
            )
            .await
            .map_err(|e| ContentPointQdrantRepositoryError::QdrantError(e.to_string()))?;

        info!("Saved content points");
        Ok(())
    }
}

#[derive(thiserror::Error)]
pub enum ContentPointQdrantRepositoryError {
    #[error("Error from Qdrant: {0}")]
    QdrantError(String),

    #[error("Error from Qdrant config: {0}")]
    QdrantConfigurationError(String),
}

impl std::fmt::Debug for ContentPointQdrantRepositoryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        error_chain_fmt(self, f)
    }
}

impl From<ContentPoint> for PointStruct {
    fn from(content_point: ContentPoint) -> Self {
        Self {
            id: Some(content_point.id.to_string().into()),
            vectors: Some(content_point.vector.into()),
            payload: content_point.payload.into(),
        }
    }
}

impl From<ContentPointPayload> for HashMap<String, qdrant::Value> {
    fn from(payload: ContentPointPayload) -> Self {
        HashMap::from([("content".into(), qdrant::Value::from(payload.content))])
    }
}
