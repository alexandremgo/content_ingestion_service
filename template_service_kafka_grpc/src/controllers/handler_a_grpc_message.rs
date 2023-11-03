use std::sync::Arc;

use shaku::HasComponent;
use sqlx::PgPool;
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
use crate::repositories::source_meta_postgres_repository::{
    PostgresRepository, SourceMetaRepository,
};
use crate::startup::DIContainer;

/// Controller manager for the RPC endpoint
///
/// An instance of this controller manager is shared between threads
///
/// The instances of needed repositories and use-cases are created for each handled request.
/// The repositories (because they can be mutated when using a transaction for ex) and use-cases (because they depend on repositories)
/// should not be shared between thread.
/// So only database connections pools or service clients that are thread-safe can be
/// properties of the controller manager.
///
/// The specific (here gRPC) controller manager knows about the repositories implementations.
pub struct SearchFulltextGrpcController {
    postgres_pool: Arc<PgPool>,
}

impl SearchFulltextGrpcController {
    pub fn new(postgres_pool: Arc<PgPool>) -> SearchFulltextGrpcController {
        SearchFulltextGrpcController { postgres_pool }
    }
}

#[tonic::async_trait]
impl FulltextSearchService for SearchFulltextGrpcController {
    #[tracing::instrument(name = "Full-text search", skip(self, request))]
    async fn search(
        &self,
        request: Request<SearchRequest>,
    ) -> Result<Response<SearchResponse>, Status> {
        info!("Search request = {:?}", request);

        let source_meta_repository = PostgresRepository::new(self.postgres_pool.clone());
        // TODO: is the Arc necessary anymore in the use case ?
        let source_meta_repository = Arc::new(source_meta_repository);
        let search_fulltext_use_case = SearchFulltextUseCase::new(source_meta_repository);

        request.
        let response = search_fulltext_use_case.execute(request.message().into());

        let response = SearchResponse {
            id: "test".to_owned(),
            content: "this is a test content".to_owned(),
            metadata: None,
        };

        Ok(Response::new(response))
    }
}

impl From<SearchRequest> for crate::domain::use_cases::search_fulltext::SearchFulltextRequest {
    fn from(value: SearchRequest) -> Self {
        crate::domain::use_cases::search_fulltext::SearchFulltextRequest {
            query: value.query,
            limit: value.limit,
        }
    }
}

/// Registers the gRPC server to a given method
#[tracing::instrument(name = "Register gRPC server for a given method", skip(di_container))]
pub async fn register_handler(
    di_container: Arc<DIContainer>,
) -> Result<(), GrpcRegisterHandlerError> {
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
