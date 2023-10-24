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
    GrpcError(#[from] tonic::transport::Error)
}

impl std::fmt::Debug for GrpcRegisterHandlerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        error_chain_fmt(self, f)
    }
}

// /// Registers the gRPC server to a given method
// #[tracing::instrument(name = "Register gRPC server for a given method")]
// pub async fn register_handler(
//     kafka_client_config: ClientConfig,
//     topic: &str,
// ) -> Result<(), GrpcRegisterHandlerError> {
//     // TODO: not failing fast enough if the broker does not exist/cannot be connected to it
//     let consumer: StreamConsumer = kafka_client_config.create()?;

//     // TODO: need a way to check and create topic
//     consumer.subscribe(&[topic])?;

//     // Create the outer pipeline on the message stream.
//     let stream_processor = consumer.stream().try_for_each(|borrowed_message| {
//         // let producer = producer.clone();
//         // let output_topic = output_topic.to_string();

//         async move {
//             // Borrowed messages can't outlive the consumer they are received from, so they need to
//             // be owned in order to be sent to a separate thread (TODO: necessary ? For heavy computation i)
//             let owned_message = borrowed_message.detach();

//             match execute_handler(&owned_message).await {
//                 Ok(()) => {
//                     info!("Success !",);
//                     // TODO: no need to ack ?
//                 }
//                 Err(error) => {
//                     error!(?error, "Failed to handle a kafka message");
//                     // TODO: no need to nack ?
//                 }
//             }

//             Ok(())
//         }
//         .instrument(info_span!(
//             "Handling consumed message",
//             message_id = %uuid::Uuid::new_v4(),
//         ))
//     });

//     info!("Starting event loop");
//     stream_processor.await?; // expect("stream processing failed");
//     info!("Stream processing terminated");

//     Ok(())
// }

// #[derive(thiserror::Error)]
// pub enum GrpcExecuteHandlerError {
//     #[error("Error while serializing message data: {0}")]
//     JsonError(#[from] serde_json::Error),
//     #[error("{0}")]
//     MessageError(String),
// }

// impl std::fmt::Debug for GrpcExecuteHandlerError {
//     fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
//         error_chain_fmt(self, f)
//     }
// }

// #[tracing::instrument(name = "Executing handler on a kafka message")]
// pub async fn execute_handler(message: &OwnedMessage) -> Result<(), GrpcExecuteHandlerError> {
//     let dto = match message.payload_view::<str>() {
//         Some(Ok(payload)) => payload,
//         Some(Err(_)) => {
//             return Err(GrpcExecuteHandlerError::MessageError(
//                 "Message payload is not a string".to_owned(),
//             ));
//         }
//         None => {
//             return Err(GrpcExecuteHandlerError::MessageError(
//                 "No payload".to_owned(),
//             ));
//         }
//     };

//     info!(?dto, "Payload len is {}", dto.len());

//     info!("Successfully handled kafka message");
//     Ok(())
// }
