use futures::TryStreamExt;
use rdkafka::{
    consumer::{Consumer, StreamConsumer},
    error::KafkaError,
    message::OwnedMessage,
    ClientConfig, Message,
};
use tracing::{error, info, info_span, Instrument};

use common::helper::error_chain_fmt;

#[derive(thiserror::Error)]
pub enum KafkaRegisterHandlerError {
    #[error(transparent)]
    KafkaError(#[from] KafkaError),
}

impl std::fmt::Debug for KafkaRegisterHandlerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        error_chain_fmt(self, f)
    }
}

/// Registers the message consumer to a given topic
#[tracing::instrument(name = "Register message handler")]
pub async fn register_handler(
    kafka_client_config: ClientConfig,
    topic: &str,
) -> Result<(), KafkaRegisterHandlerError> {
    // TODO: not failing fast enough if the broker does not exist/cannot be connected to it
    let consumer: StreamConsumer = kafka_client_config.create()?;

    // TODO: need a way to check and create topic
    consumer.subscribe(&[topic])?;

    // Create the outer pipeline on the message stream.
    let stream_processor = consumer.stream().try_for_each(|borrowed_message| {
        // let producer = producer.clone();
        // let output_topic = output_topic.to_string();

        async move {
            // Borrowed messages can't outlive the consumer they are received from, so they need to
            // be owned in order to be sent to a separate thread (TODO: necessary ? For heavy computation i)
            let owned_message = borrowed_message.detach();

            match execute_handler(&owned_message).await {
                Ok(()) => {
                    info!("Success !",);
                    // TODO: no need to ack ?
                }
                Err(error) => {
                    error!(?error, "Failed to handle a kafka message");
                    // TODO: no need to nack ?
                }
            }

            Ok(())
        }
        .instrument(info_span!(
            "Handling consumed message",
            message_id = %uuid::Uuid::new_v4(),
        ))
    });

    info!("Starting event loop");
    stream_processor.await?; // expect("stream processing failed");
    info!("Stream processing terminated");

    Ok(())
}

#[derive(thiserror::Error)]
pub enum KafkaExecuteHandlerError {
    #[error("Error while serializing message data: {0}")]
    JsonError(#[from] serde_json::Error),
    #[error("{0}")]
    MessageError(String),
}

impl std::fmt::Debug for KafkaExecuteHandlerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        error_chain_fmt(self, f)
    }
}

#[tracing::instrument(name = "Executing handler on a kafka message")]
pub async fn execute_handler(message: &OwnedMessage) -> Result<(), KafkaExecuteHandlerError> {
    let dto = match message.payload_view::<str>() {
        Some(Ok(payload)) => payload,
        Some(Err(_)) => {
            return Err(KafkaExecuteHandlerError::MessageError(
                "Message payload is not a string".to_owned(),
            ));
        }
        None => {
            return Err(KafkaExecuteHandlerError::MessageError("No payload".to_owned()));
        }
    };

    info!(?dto, "Payload len is {}", dto.len());

    info!("Successfully handled kafka message");
    Ok(())
}
