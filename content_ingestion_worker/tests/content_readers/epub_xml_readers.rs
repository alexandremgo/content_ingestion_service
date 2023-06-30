use std::io::BufReader;
use std::io::Write;

use content_ingestion_worker::{
    domain::entities::epub_source_reader, domain::{services::extract_content_from_xml::{self, extract_content_from_xml}, entities::epub_source_reader::EpubSourceReader},
};
use genawaiter::GeneratorState;

#[test]
fn on_correct_epub_it_extracts_contents() {
    let mut source_buffer =
        EpubSourceReader::try_new(String::from("tests/resources/accessible_epub_3.epub")).unwrap();

    let buf_reader = BufReader::new(source_buffer);
    let nb_words_per_document = 100;

    let mut generator = extract_content_from_xml(buf_reader, Some(nb_words_per_document));
    let mut completed = false;

    // Test locally
    // let mut file = std::fs::OpenOptions::new()
    //     .create(true)
    //     .append(true)
    //     .open("tests/resources/results_3_bytes_accessible.txt")
    //     .unwrap();

    // while !completed {
    //     let extracted_content = match generator.as_mut().resume() {
    //         GeneratorState::Yielded(content) => content,
    //         GeneratorState::Complete(_result) => {
    //             completed = true;
    //             String::from("")
    //         }
    //     };

    //     if let Err(e) = writeln!(file, "{}\n----\n", extracted_content) {
    //         eprintln!("Couldn't write to file: {}", e);
    //     }
    // }
}
