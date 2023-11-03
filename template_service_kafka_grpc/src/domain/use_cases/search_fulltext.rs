use std::sync::Arc;

use crate::repositories::source_meta_postgres_repository::SourceMetaRepository;

pub struct SearchFulltextRequest {
    pub query: String,
    /// TODO: JSON-like metadata properties ? Needs to define exact properties, or HashMap
    // pub metadata: ::core::option::Option<Metadata>,
    pub limit: u32,
}

pub struct SearchFulltextResponse {

}

pub struct SearchFulltextError {

}

/// TODO: first trying with only source meta repository. Then try with meilisearch content repository.
pub struct SearchFulltextUseCase {
  source_meta_repository: Arc<dyn SourceMetaRepository>,
}

impl SearchFulltextUseCase {
  /// TODO: is the Arc necessary ? We are not sharing the use cases between thread
  pub fn new(source_meta_repository: Arc<dyn SourceMetaRepository>) -> SearchFulltextUseCase {
    SearchFulltextUseCase {
      source_meta_repository
    }
  }

  pub fn execute(&self, request: &SearchFulltextRequest) -> Result<SearchFulltextResponse, SearchFulltextError> {
    Ok(SearchFulltextResponse { })
  }
}
