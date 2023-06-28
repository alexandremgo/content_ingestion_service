use crate::helper::error_chain_fmt;
use s3::Bucket;
use tracing::{error, info};

/// Simple Storage Service (S3) client to store source files
pub struct S3Repository {
    // If one day there is a need to have several buckets for scaling reasons,
    // a vector of Bucket will be necessary + knowing in which bucket each file is
    bucket: Bucket,
}

#[derive(thiserror::Error)]
pub enum S3RepositoryError {
    #[error("The object could not be found in the bucket: {0}")]
    ObjectNotFound(String),
    #[error(transparent)]
    IOError(#[from] std::io::Error),
    #[error(transparent)]
    Other(#[from] s3::error::S3Error),
}

impl std::fmt::Debug for S3RepositoryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        error_chain_fmt(self, f)
    }
}

impl S3Repository {
    pub fn new(bucket: Bucket) -> Self {
        Self { bucket }
    }

    /// Get a given stream of a file from a bucket in the object storage
    ///
    /// WIP - not sure how to transform this into a buffer reader
    ///
    /// # Arguments
    /// * `object_path_name` - The path (with the object name) of the file to get
    ///
    /// # Return
    /// A stream of Bytes from the S3 bucket
    // #[tracing::instrument(name = "Get file from bucket", skip(self))]
    // pub async fn get_file_stream(
    //     &self,
    //     object_path_name: &str,
    // ) -> Result<Pin<Box<dyn Stream<Item = Bytes>>>, S3RepositoryError> {

    //     let stream = self.bucket.get_object_stream(object_path_name).await?;
    //     // Check stream status
    //     info!("Stream status: {}", stream.status_code);

    //     Ok(stream.bytes)
    // }

    /// Get a file from a bucket in the object storage
    ///
    /// # Arguments
    /// * `object_path_name` - The path (with the object name) of the file to get
    ///
    /// # Return
    /// The name (not the full path) of the file given on the object storage
    #[tracing::instrument(name = "Get file from bucket", skip(self))]
    pub async fn get_file(&self, object_path_name: &str) -> Result<Vec<u8>, S3RepositoryError> {
        let response = self.bucket.get_object(object_path_name).await?;
        // Check stream status
        info!("🦄 Get from bucket response: {}", response.status_code());

        Ok(response.to_vec())
    }
}
