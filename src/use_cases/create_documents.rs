use std::io::{BufRead, BufReader};

use crate::domain::entities::document::Document;
use crate::domain::entities::epub_file::EpubFile;

const DEFAULT_NB_WORDS_PER_DOCUMENT: usize = 200;

// Q: what does the EpubFile look like ?
// We will want to read the file line by line - as it is more efficient
// So we can't have a "content" field in EpubFile

pub struct CreateDocumentsArgs {
    epub_file: EpubFile,
    nb_words_per_document: Option<usize>,
}

pub fn create_documents(args: CreateDocumentsArgs) -> Result<(), ()> {
    let CreateDocumentsArgs { epub_file, nb_words_per_document } = args;
    let nb_words_per_document = nb_words_per_document.unwrap_or(DEFAULT_NB_WORDS_PER_DOCUMENT);

    // For the actual API: will we save the epub file when receiving it ?
    // For now, no repository for EpubFile. We will just read it from the filesystem
    let mut file = std::fs::File::open(epub_file.path).unwrap();

    let file_reader = BufReader::new(file);
    let mut lines_iter = file_reader.lines();

    // Without having a state machine, we can't know if we are in the body or not

    // Could give the BufReader to the services extract_content, and let it handle the state machine 
    // It would follow a generator pattern: yielding the content, each time it reaches nb_words_per_document

    // Should i build an aggregate entity that would create all the documents ?
    Ok(())
}
