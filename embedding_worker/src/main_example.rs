use rust_bert::pipelines::sentence_embeddings::{SentenceEmbeddingsBuilder, SentenceEmbeddingsModelType};

fn main() {
    println!("Hello, world!");
    let model = SentenceEmbeddingsBuilder::remote(SentenceEmbeddingsModelType::AllMiniLmL12V2)
        .create_model().unwrap();

    let sentences = ["this is an example sentence", "each sentence is converted"];

    let output = model.encode(&sentences).unwrap();

    println!("🦄 Output:\n {:?}", output);
}
