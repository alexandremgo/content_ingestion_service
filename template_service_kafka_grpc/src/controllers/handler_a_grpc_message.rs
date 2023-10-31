use std::sync::Arc;

use shaku::HasComponent;
use tonic::transport::Server;
use tonic::{Request, Response, Status};
use tracing::{error, info, info_span, Instrument};

use common::helper::error_chain_fmt;

use common::dtos::proto::fulltext_search_service::{
    fulltext_search_service_server::{FulltextSearchService, FulltextSearchServiceServer},
    SearchRequest, SearchResponse,
};

use crate::domain::use_cases::search_fulltext::SearchFulltextUseCase;
use crate::ports::content_repository::ContentRepository;
use crate::repositories::source_meta_postgres_repository::SourceMetaRepository;
use crate::startup::DIContainer;

pub struct SearchFulltextGrpcController {
    // search_fulltext_use_case: SearchFulltextUseCase,
}

impl SearchFulltextGrpcController {
    pub fn new(source_meta_repository: Arc<dyn SourceMetaRepository>) -> SearchFulltextGrpcController {
        let search_fulltext_use_case = SearchFulltextUseCase::new(source_meta_repository);

        // TODO: here we would pass the implementation of each repository etc. ?
        // Repositories need to handle their own pool of connection ?
        SearchFulltextGrpcController {
            // search_fulltext_use_case
        }
    }

    // pub fn init_use_cases() ?
    // TODO: do we need to init a use case at each request ? because gRPC keeps the controller instance alive and use
    // it for each request ?
}

#[tonic::async_trait]
impl FulltextSearchService for SearchFulltextGrpcController {
    #[tracing::instrument(name = "Full-text search", skip(self, request))]
    async fn search(
        &self,
        request: Request<SearchRequest>,
    ) -> Result<Response<SearchResponse>, Status> {
        info!("Search request = {:?}", request);

        let response = SearchResponse {
            id: "test".to_owned(),
            content: "this is a test content".to_owned(),
            metadata: None,
        };

        Ok(Response::new(response))
    }
}

/// Registers the gRPC server to a given method
#[tracing::instrument(name = "Register gRPC server for a given method", skip(di_container))]
pub async fn register_handler(di_container: Arc<DIContainer>) -> Result<(), GrpcRegisterHandlerError> {
// #[tracing::instrument(name = "Register gRPC server for a given method", skip(content_repository))]
// pub async fn register_handler(content_repository: Arc<dyn ContentRepository>) -> Result<(), GrpcRegisterHandlerError> {
    // let addr = "[::1]:10000".parse().unwrap();

    // println!("RouteGuideServer listening on: {}", addr);
    // let content_repository: &dyn ContentRepository = di_container.resolve_ref();

    // let controller = SearchFulltextGrpcController::new(content_repository);

    // let server = FulltextSearchServiceServer::new(controller);

    // Server::builder().add_service(server).serve(addr).await?;

    Ok(())
}

#[derive(thiserror::Error)]
pub enum GrpcRegisterHandlerError {
    #[error(transparent)]
    GrpcError(#[from] tonic::transport::Error),
}

impl std::fmt::Debug for GrpcRegisterHandlerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        error_chain_fmt(self, f)
    }
}
