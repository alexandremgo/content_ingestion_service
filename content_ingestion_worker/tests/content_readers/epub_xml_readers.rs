use chrono::Utc;
use genawaiter::GeneratorState;
use serde_json::json;
use std::io::BufReader;
use std::io::Write;
use uuid::Uuid;

use content_ingestion_worker::domain::entities::{epub_reader::EpubReader, xml_reader};
use content_ingestion_worker::domain::services::extract_content_generator::extract_content_generator;

use crate::helpers::init_test;

#[test]
fn on_correct_epub_it_should_be_able_to_extract_expected_contents() {
    init_test();

    let file_name = "minimal_sample.epub";
    let file = std::fs::File::open(format!("tests/resources/{}", file_name)).unwrap();
    let file_reader = BufReader::new(file);

    let epub_reader =
        EpubReader::from_reader(file_reader, Some(json!({ "book": file_name }))).unwrap();
    let mut xml_reader = xml_reader::build_from_reader(epub_reader);

    let nb_words_per_content = 100;
    let mut generator = extract_content_generator(&mut xml_reader, Some(nb_words_per_content));

    let mut is_extraction_completed = false;

    // Writes result locally for observations
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(format!(
            "tests/resources/results/results_epub_xml_readers_{}_{}.txt",
            Utc::now().format("%Y-%m-%d_%H-%M-%S"),
            Uuid::new_v4().to_string()
        ))
        .unwrap();

    let mut i = 0;

    // Limits to avoid infinite loop during tests
    // It should never reach 1000 extracted contents in this test.
    while i < 1000 {
        let extracted_content = match generator.as_mut().resume() {
            GeneratorState::Yielded(content) => content,
            GeneratorState::Complete(_result) => {
                is_extraction_completed = true;
                break;
            }
        };

        // On this integrations tests, we are not currently checking every extracted content as the readers implementation might still change.
        // Writes on a result file to be able to check it later.
        if let Err(e) = writeln!(
            file,
            "{}: {}\n\n{}\n----\n\n",
            i, extracted_content.metadata, extracted_content.content
        ) {
            eprintln!("Couldn't write to file: {}", e);
        }

        match i {
            0 => {
                assert!(extracted_content.content.contains("Lorem ipsum dolor sit amet, consectetur adipiscing elit. Morbi id lectus dictum, lobortis urna a"));
                assert_eq!(
                    extracted_content.metadata["xml"]["title"],
                    "chapter_1.xhtml"
                );
                assert_eq!(extracted_content.metadata["epub"]["chapter_number"], 1);
                assert_eq!(
                    extracted_content.metadata["epub"]["book"],
                    "minimal_sample.epub"
                );
            }
            7 => {
                assert!(extracted_content.content.contains("Lorem ipsum dolor sit amet, consectetur adipiscing elit. Maecenas id ex urna. Quisque at fringilla ex."));
                assert_eq!(
                    extracted_content.metadata["xml"]["title"],
                    "chapter_2.xhtml"
                );
                assert_eq!(extracted_content.metadata["epub"]["chapter_number"], 2);
                assert_eq!(
                    extracted_content.metadata["epub"]["book"],
                    "minimal_sample.epub"
                );
            }
            _ => {}
        }

        i += 1;
    }

    assert!(is_extraction_completed);
}
