use uuid::Uuid;

#[derive(Debug, Clone, serde::Serialize)]
pub enum SourceType {
    Epub,
}

#[derive(Debug, serde::Serialize)]
pub struct ExtractContentJob {
    /// Id of the source meta associated to the file the job is working on
    pub source_meta_id: Uuid,

    /// Path and name of the file saved in the object store
    pub object_store_path_name: String,

    /// Type of source file
    pub source_type: SourceType,
}
