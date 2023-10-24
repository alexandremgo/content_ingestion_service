use tonic::transport::Server;
use tonic::{Request, Response, Status};
use tracing::{error, info, info_span, Instrument};

use common::helper::error_chain_fmt;

use common::dtos::proto::fulltext_search_service::{
    fulltext_search_service_server::{FulltextSearchService, FulltextSearchServiceServer},
    SearchRequest, SearchResponse,
};

pub struct SearchController {}

#[tonic::async_trait]
impl FulltextSearchService for SearchController {
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
#[tracing::instrument(name = "Register gRPC server for a given method")]
pub async fn register_handler() -> Result<(), GrpcRegisterHandlerError> {
    let addr = "[::1]:10000".parse().unwrap();

    println!("RouteGuideServer listening on: {}", addr);

    let controller = SearchController {};

    let server = FulltextSearchServiceServer::new(controller);

    Server::builder().add_service(server).serve(addr).await?;

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
