use common::telemetry::{get_tracing_subscriber, init_tracing_subscriber};
use embedding_worker::{configuration::get_configuration, startup::Application};

use tracing::info;

use std::{
    io,
    sync::mpsc,
    thread::{self, JoinHandle},
};

use anyhow::Result;
use rust_bert::pipelines::sentence_embeddings::{
    SentenceEmbeddingsBuilder, SentenceEmbeddingsModelType,
};
use tokio::{sync::oneshot, task};

#[tokio::main]
async fn main() -> Result<()> {
    let tracing_subscriber =
        get_tracing_subscriber("embedding_worker".into(), "info".into(), std::io::stdout);
    init_tracing_subscriber(tracing_subscriber);

    let (_handle, embeddings_generator) = EmbeddingsGenerator::spawn();

    loop {
        let mut sentence = String::new();

        io::stdin()
            .read_line(&mut sentence)
            .expect("Failed to read line");

        let embeddings = embeddings_generator
            .generate_embeddings(vec![sentence])
            .await?;

        println!("\n\nResults: {embeddings:?}\n\n\n-------------------\n\n\n");
    }

    Ok(())
}

type Embeddings = Vec<f32>;
/// Message type for internal channel, passing around input sentences and generated embeddings
type Message = (Vec<String>, oneshot::Sender<Vec<Embeddings>>);

/// Runner for embeddings generator
#[derive(Debug, Clone)]
pub struct EmbeddingsGenerator {
    sender: mpsc::SyncSender<Message>,
}

impl EmbeddingsGenerator {
    /// Spawn a embeddings model on a separate thread and return an embeddings generator instance
    /// to interact with it
    pub fn spawn() -> (JoinHandle<Result<()>>, EmbeddingsGenerator) {
        let (sender, receiver) = mpsc::sync_channel(100);
        let handle = thread::spawn(move || Self::runner(receiver));
        (handle, EmbeddingsGenerator { sender })
    }

    /// The classification runner itself
    /// Needs to be in sync runtime, async doesn't work
    #[tracing::instrument(name = "🏃‍♂️ Runner", skip(receiver))]
    fn runner(receiver: mpsc::Receiver<Message>) -> Result<()> {
        // let model = SentimentModel::new(SentimentConfig::default())?;

        info!("🏃‍♂️ Loading model");
        let model = SentenceEmbeddingsBuilder::remote(SentenceEmbeddingsModelType::AllMiniLmL12V2)
            .create_model()?;
        info!("🏃‍♂️ Model loaded");

        while let Ok((sentences, sender)) = receiver.recv() {
            info!("🏃‍♂️ Received sentences to work on: {:?}", sentences);

            let sentences: Vec<&str> = sentences.iter().map(String::as_str).collect();
            let embeddings = model.encode(&sentences)?;
            // let sentiments = model.predict(texts);
            sender.send(embeddings).expect("sending embeddings");
            // sender.send(sentiments).expect("sending results");
        }

        Ok(())
    }

    /// Makes the runner generate embeddings on sentences and returns the result
    pub async fn generate_embeddings(&self, sentences: Vec<String>) -> Result<Vec<Embeddings>> {
        let (sender, receiver) = oneshot::channel();
        task::block_in_place(|| self.sender.send((sentences, sender)))?;
        Ok(receiver.await?)
    }
}
