use crate::domain::{entities::content_point::Embeddings, services::helpers::split_sentences};
use common::helper::error_chain_fmt;
use rust_bert::{
    pipelines::sentence_embeddings::{SentenceEmbeddingsBuilder, SentenceEmbeddingsModelType},
    RustBertError,
};
use std::{
    sync::mpsc,
    thread::{self, JoinHandle},
};
use tokio::{sync::oneshot, task};
use tracing::{debug, info};

/// Service to generate embeddings from a text content, using models available from Hugging Face.
///
/// Using model AllMiniLmL12V2
///
/// Question: should it be considered a "repository" ?
pub struct HuggingFaceEmbeddingsService {
    sender_to_runner: mpsc::SyncSender<RunnerMessage>,
    _thread_handle: JoinHandle<Result<(), HuggingFaceEmbeddingsServiceError>>,
}

impl HuggingFaceEmbeddingsService {
    /// Spawns an embeddings generator runner on a separate thread
    /// and returns an `EmbeddingsGenerator` to interact with the runner
    pub fn new() -> Self {
        let (sender, receiver) = mpsc::sync_channel(100);
        let handle = thread::spawn(move || Self::runner(receiver));

        Self {
            _thread_handle: handle,
            sender_to_runner: sender,
        }
    }

    /// The embeddings generator runner itself
    ///
    /// As running extensive calculations like running embeddings generation in a future should be avoided,
    /// the runner needs to be in sync runtime.
    ///
    /// The message received by this runner contains the sentences to work on
    /// and a sender to communicate the resulting embeddings
    ///
    /// Currently using all-MiniLM-L12-v2: maps sentences to a 384 dimensional dense vector space
    #[tracing::instrument(name = "Runner", skip(receiver))]
    fn runner(
        receiver: mpsc::Receiver<RunnerMessage>,
    ) -> Result<(), HuggingFaceEmbeddingsServiceError> {
        let model = SentenceEmbeddingsBuilder::remote(SentenceEmbeddingsModelType::AllMiniLmL12V2)
            .create_model()?;
        info!("Embeddings model loaded ✅");

        while let Ok((sentences, sender)) = receiver.recv() {
            let sentences: Vec<&str> = sentences.iter().map(String::as_str).collect();
            let embeddings = model.encode(&sentences)?;

            sender.send(embeddings).expect("sending embeddings");
        }

        Ok(())
    }

    #[tracing::instrument(name = "Generate embeddings", skip(self))]
    pub async fn generate_embeddings(
        &self,
        content: &str,
    ) -> Result<Vec<Embeddings>, HuggingFaceEmbeddingsServiceError> {
        // A content could have one or more sentences
        let sentences = split_sentences(content);
        debug!(?sentences, "Splitted content");

        let (sender, receiver) = oneshot::channel();

        task::block_in_place(|| self.sender_to_runner.send((sentences, sender)))?;

        Ok(receiver.await?)
    }
}

#[derive(thiserror::Error)]
pub enum HuggingFaceEmbeddingsServiceError {
    #[error("Embeddings model error: {0}")]
    ModelError(#[from] RustBertError),
    #[error(transparent)]
    SenderError(
        #[from]
        std::sync::mpsc::SendError<(
            Vec<std::string::String>,
            tokio::sync::oneshot::Sender<Vec<Embeddings>>,
        )>,
    ),
    #[error(transparent)]
    ReceiverError(#[from] tokio::sync::oneshot::error::RecvError),
}

impl std::fmt::Debug for HuggingFaceEmbeddingsServiceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        error_chain_fmt(self, f)
    }
}

/// Message type for internal channel, passing around input sentences and generated embeddings
type RunnerMessage = (Vec<String>, oneshot::Sender<Vec<Embeddings>>);
