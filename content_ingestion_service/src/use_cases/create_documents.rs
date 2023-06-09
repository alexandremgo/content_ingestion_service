use std::fs::OpenOptions;
use std::io::BufReader;
use std::io::Write;

use genawaiter::GeneratorState;

use crate::domain::entities::epub_file::EpubFile;
use crate::domain::services::extract_content_generator::extract_content_generator;

const DEFAULT_NB_WORDS_PER_DOCUMENT: usize = 200;

// Q: what does the EpubFile look like ?
// We will want to read the file line by line - as it is more efficient
// So we can't have a "content" field in EpubFile

pub struct CreateDocumentsArgs {
    epub_file: EpubFile,
    nb_words_per_document: Option<usize>,
}

pub fn create_documents(args: CreateDocumentsArgs) -> Result<(), anyhow::Error> {
    let CreateDocumentsArgs {
        epub_file,
        nb_words_per_document,
    } = args;
    let nb_words_per_document = nb_words_per_document.unwrap_or(DEFAULT_NB_WORDS_PER_DOCUMENT);

    // TODO: For the actual API: will we save the epub file when receiving it ?
    // For now, no repository for EpubFile. We will just read it from the filesystem
    // TODO: we're opening the EPUB as a file , is this what we want ?
    let file = std::fs::File::open(epub_file.path).unwrap();

    let buf_reader = BufReader::new(file);

    // Without having a state machine, we can't know if we are in the body or not

    // ? Could give the BufReader to the services extract_content, and let it handle the state machine
    // It would follow a generator pattern: yielding the content, each time it reaches nb_words_per_document
    let mut generator = extract_content_generator(buf_reader, Some(nb_words_per_document));
    let mut completed = false;

    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open("src/tests/results.txt")
        .unwrap();

    while !completed {
        let extracted_content = match generator.as_mut().resume() {
            GeneratorState::Yielded(content) => content,
            GeneratorState::Complete(_result) => {
                completed = true;
                String::from("")
            }
        };

        if let Err(e) = writeln!(file, "{}\n----\n", extracted_content) {
            eprintln!("Couldn't write to file: {}", e);
        }
    }

    // Should i build an aggregate entity that would create all the documents ?
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::domain::entities::epub_file::EpubFile;
    use crate::use_cases::create_documents::{create_documents, CreateDocumentsArgs};

    #[test]
    fn test_working() {
        let epub_file = EpubFile {
            book_id: String::from(""),
            path: String::from("src/tests/accessible_epub_3.epub"),
        };

        if let Err(e) = create_documents(CreateDocumentsArgs {
            epub_file,
            nb_words_per_document: Some(8),
        }) {
            panic!("Error: {:?}", e);
        }
        assert_eq!(1, 1);
    }
}
