use std::sync::Arc;

use crate::repositories::source_meta_postgres_repository::SourceMetaRepository;

pub struct SearchFulltextRequest {

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
  pub fn new(source_meta_repository: Arc<dyn SourceMetaRepository>) -> SearchFulltextUseCase {
    SearchFulltextUseCase {
      source_meta_repository
    }
  }

  pub fn execute(&self, request: &SearchFulltextRequest) -> Result<SearchFulltextResponse, SearchFulltextError> {
    Ok(SearchFulltextResponse { })
  }
}
