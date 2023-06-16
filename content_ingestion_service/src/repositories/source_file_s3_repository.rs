use std::io::Read;

use s3::{creds::Credentials, Bucket, BucketConfiguration, Region};
use secrecy::{ExposeSecret, Secret};
use tracing::{error, info};

use crate::{configuration::ObjectStorageSettings, helper::error_chain_fmt};

pub struct S3Repository {
    access_key: String,
    // To keep the credentials secret and avoids leaks in logs, we use Secret<String>
    // and the s3::creds::Credentials is created on demand
    secret_key: Secret<String>,
    config: BucketConfiguration,
    region: Region,
}

/// Simple Storage Service (S3) client to store source files
impl S3Repository {
    pub fn new(settings: &ObjectStorageSettings) -> Self {
        let region = Region::Custom {
            region: settings.region.to_owned(),
            endpoint: settings.endpoint(),
        };

        Self {
            access_key: settings.username.to_owned(),
            secret_key: settings.password.to_owned(),
            config: BucketConfiguration::default(),
            region,
        }
    }

    pub fn try_get_credentials(&self) -> Result<Credentials, S3RepositoryError> {
        let credentials = Credentials::new(
            Some(&self.access_key),
            Some(self.secret_key.expose_secret()),
            None,
            None,
            None,
        )?;

        Ok(credentials)
    }
}

#[derive(thiserror::Error)]
pub enum S3RepositoryError {
    #[error("Credentials error: {0}")]
    CredentialsError(#[from] s3::creds::error::CredentialsError),
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
    #[tracing::instrument(name = "Get or create bucket", skip(self))]
    pub async fn get_or_create_bucket(
        &self,
        bucket_name: &str,
    ) -> Result<Bucket, S3RepositoryError> {
        // Instantiates/gets the bucket if it exists
        let bucket = Bucket::new(
            bucket_name,
            self.region.to_owned(),
            self.try_get_credentials()?,
        )?
        .with_path_style();

        // Checks if the bucket exist
        if let Err(error) = bucket.head_object("/").await {
            // Only continues if the error is a bucket not found (404)
            match error {
                s3::error::S3Error::Http(code, _) => {
                    if code != 404 {
                        return Err(S3RepositoryError::Other(error));
                    }
                }
                _ => return Err(S3RepositoryError::Other(error)),
            }

            info!("Unknown bucket {}, creating it ...", bucket_name);
            Bucket::create_with_path_style(
                bucket_name,
                self.region.to_owned(),
                self.try_get_credentials()?,
                self.config.to_owned(),
            )
            .await?;
        }

        info!("Bucket {} has been correctly instantiated", bucket_name);
        Ok(bucket)
    }

    /// Save a given file to a bucket in the object storage
    ///
    /// # Arguments
    /// * `bucket` - The bucket where the file will be stored
    /// * `file` - The file to be stored
    ///
    /// # Return
    /// The name of the file given on the object storage
    #[tracing::instrument(name = "Add file from bucket", skip(self, bucket))]
    pub async fn save_file_to_bucket(
        &self,
        bucket: &Bucket,
        file: &mut std::fs::File,
    ) -> Result<String, S3RepositoryError> {
        // TODO: get source file extension
        let object_name = uuid::Uuid::new_v4();

        let mut buf = Vec::<u8>::new();
        file.read_to_end(&mut buf)?;

        bucket
            .put_object(object_name.to_string(), buf.as_slice())
            .await?;

        Ok(object_name.to_string())
    }

    /// Remove a given file from a bucket in the object storage
    ///
    /// # Arguments
    /// * `bucket` - The bucket where the file is stored
    /// * `object_name` - The name of the file to be removed
    ///
    /// # Return
    ///
    #[tracing::instrument(name = "Remove file from bucket", skip(self, bucket))]
    pub async fn remove_file_from_bucket(
        &self,
        bucket: &Bucket,
        object_name: &str,
    ) -> Result<(), S3RepositoryError> {
        bucket
            .delete_object(&object_name)
            .await
            .map_err(|error| match error {
                s3::error::S3Error::Http(code, _) => {
                    if code == 404 {
                        return S3RepositoryError::ObjectNotFound(object_name.to_string());
                    }
                    S3RepositoryError::Other(error)
                }
                _ => S3RepositoryError::Other(error),
            })?;

        Ok(())
    }
}
