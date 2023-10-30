use std::sync::Arc;

use crate::ports::content_repository::ContentRepository;


pub struct SearchFulltextRequest {

}

pub struct SearchFulltextResponse {

}

pub struct SearchFulltextError {

}

pub struct SearchFulltextUseCase {
  content_repository: Arc<dyn ContentRepository>,
}

impl SearchFulltextUseCase {
  pub fn new(content_repository: Arc<dyn ContentRepository>) -> SearchFulltextUseCase {
    SearchFulltextUseCase {
      content_repository
    }
  }

  pub fn execute(&self, request: &SearchFulltextRequest) -> Result<SearchFulltextResponse, SearchFulltextError> {
    Ok(SearchFulltextResponse { })
  }
}
