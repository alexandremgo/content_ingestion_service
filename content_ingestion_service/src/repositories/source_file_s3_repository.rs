use s3::{creds::Credentials, Bucket, BucketConfiguration, Region};
use tracing::{error, info};

use crate::helper::error_chain_fmt;

// TODO: create a Repository struct, with credentials, region and other info ?

// Could be a repository port/interface
fn save_file_in_bucket(bucket_name: String) -> Result<(), ()> {
    Ok(())
}

#[derive(thiserror::Error)]
pub enum S3RepositoryError {
    #[error("Credentials error: {0}")]
    CredentialsError(#[from] s3::creds::error::CredentialsError),
    #[error("The bucket {0} was not found")]
    BucketNotFound(String),
    #[error("The bucket {0} could not be created")]
    BucketCreationError(String),
    #[error(transparent)]
    Other(#[from] s3::error::S3Error),
}

impl std::fmt::Debug for S3RepositoryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        error_chain_fmt(self, f)
    }
}

#[tracing::instrument(name = "Get or create bucket")]
pub async fn get_or_create_bucket(bucket_name: &str) -> Result<Bucket, S3RepositoryError> {
    //  eu-fr-1
    // TODO: in struct
    let region = Region::Custom {
        region: "eu-fr-1".to_owned(),
        endpoint: "http://127.0.0.1:9000".to_owned(),
    };

    // TODO: in struct
    let credentials = Credentials::new(Some("minio"), Some("password"), None, None, None)?;
    let config = BucketConfiguration::default();

    // Instantiates/gets the bucket if it exists
    let bucket = Bucket::new(&bucket_name, region, credentials.clone())?.with_path_style();

    // let (_, code) = bucket.head_object("/").await?;
    // if code == 404 {
    // let _: Result<(), ()> = match bucket.head_object("/").await {
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
        Bucket::create_with_path_style(&bucket_name, bucket.region.clone(), credentials, config)
            .await?;
    }

    info!("Bucket {} has been correctly instantiated", bucket_name);
    Ok(bucket)
}
