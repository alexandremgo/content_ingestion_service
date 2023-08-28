use crate::helper::error_chain_fmt;
use s3::Bucket;
use std::io::Read;
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

    /// Save a given file to a bucket in the object storage
    ///
    /// # Arguments
    /// * `file` - The file to be stored
    /// * `folder_path` - The folder where the file will be stored
    ///
    /// # Return
    /// A tuple:
    /// - the name (not the full path) of the file given on the object storage
    /// - the path + name (full path) of the file given on the object storage
    #[tracing::instrument(name = "Add file from bucket", skip(self))]
    pub async fn save_file(
        &self,
        folder_path: &str,
        file: &mut std::fs::File,
    ) -> Result<(String, String), S3RepositoryError> {
        let object_name = uuid::Uuid::new_v4();
        let object_path_name = format!("{}/{}", folder_path, object_name);

        info!("Saving file at {}", object_path_name);

        let mut buf = Vec::<u8>::new();
        file.read_to_end(&mut buf)?;

        self.bucket
            .put_object(object_path_name.clone(), buf.as_slice())
            .await?;

        Ok((object_name.to_string(), object_path_name))
    }

    /// Remove a given file from a bucket in the object storage
    ///
    /// # Arguments
    /// * `bucket` - The bucket where the file is stored
    /// * `object_path` - The path (with the object name) of the file that should be removed
    #[tracing::instrument(name = "Remove file from bucket", skip(self))]
    pub async fn remove_file(&self, object_path: &str) -> Result<(), S3RepositoryError> {
        self.bucket
            .delete_object(&object_path)
            .await
            .map_err(|error| match error {
                s3::error::S3Error::Http(code, _) => {
                    if code == 404 {
                        return S3RepositoryError::ObjectNotFound(object_path.to_string());
                    }
                    S3RepositoryError::Other(error)
                }
                _ => S3RepositoryError::Other(error),
            })?;

        Ok(())
    }
}
