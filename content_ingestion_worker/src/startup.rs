use lapin::{
    message::DeliveryResult,
    options::{BasicAckOptions, BasicConsumeOptions, QueueDeclareOptions},
    types::FieldTable,
    Channel, Connection,
};
use tracing::{error, info, info_span, Instrument};

use crate::{
    configuration::{RabbitMQSettings, Settings},
    handlers::example,
};

/// Holds the newly built RabbitMQ connection and any server/useful properties
pub struct Application {
    rabbitmq_connection: Connection,
    rabbitmq_queue_name_prefix: String,
}

impl Application {
    pub async fn build(configuration: Settings) -> Result<Self, ()> {
        let connection = get_rabbitmq_connection(&configuration.rabbitmq).await;

        Ok(Self {
            rabbitmq_connection: connection,
            rabbitmq_queue_name_prefix: configuration.rabbitmq.queue_name_prefix,
        })
    }

    pub async fn create_rabbitmq_channel(&self) -> Channel {
        self.rabbitmq_connection.create_channel().await.unwrap()
    }

    pub async fn run(&self) -> Result<(), std::io::Error> {
        // A channel is a lightweight connection that share a single TCP connection to RabbitMQ
        let channel = self.rabbitmq_connection.create_channel().await.unwrap();

        let queue_name = format!("{}_queue_test", self.rabbitmq_queue_name_prefix);
        info!("🏗️ Declaring queue: {}", queue_name);

        let _queue = channel
            .queue_declare(
                &queue_name,
                QueueDeclareOptions::default(),
                FieldTable::default(),
            )
            .await
            .unwrap();

        let consumer = channel
            .basic_consume(
                "queue_test",
                "tag_foo",
                BasicConsumeOptions::default(),
                FieldTable::default(),
            )
            .await
            .unwrap();

        // TODO: will need to set this in another way
        consumer.set_delegate(move |delivery: DeliveryResult| {
            async move {
                let delivery = match delivery {
                    // Carries the delivery alongside its channel
                    Ok(Some(delivery)) => delivery,
                    // The consumer got canceled
                    Ok(None) => return,
                    // Carries the error and is always followed by Ok(None)
                    Err(error) => {
                        error!(?error, "Failed to consume queue message");
                        return;
                    }
                };

                let my_data = match example::MyData::try_parsing(&delivery.data) {
                    Ok(my_data) => my_data,
                    Err(error) => {
                        error!(?error, "Failed to parse queue message data: {}", error);
                        return;
                    }
                };

                info!(
                    "🦖 Received message properties: {:#?}\n",
                    delivery.properties
                );

                match example::handler(my_data) {
                    Ok(()) => (),
                    Err(error) => {
                        error!(?error, "Failed to handle queue message");
                        return;
                    }
                }

                delivery
                    .ack(BasicAckOptions::default())
                    .await
                    .expect("Failed to ack send_webhook_event message");
            }
            .instrument(info_span!(
                "Handling queued message",
                handler_id = %uuid::Uuid::new_v4()
            ))
        });

        Ok(())
    }
}

async fn get_rabbitmq_connection(config: &RabbitMQSettings) -> Connection {
    Connection::connect(&config.get_uri(), config.get_connection_properties())
        .await
        .unwrap()
}
