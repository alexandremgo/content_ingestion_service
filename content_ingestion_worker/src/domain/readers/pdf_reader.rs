use std::io::Read;

use lopdf::Document;
use serde_json::{json, Map, Value as JsonValue};
use tracing::info;

use crate::{domain::entities::meta_read::MetaRead, helper::error_chain_fmt};

const PDF_READER_META_KEY: &str = "pdf";
const PDF_READER_META_KEY_DEFAULT_INITIAL: &str = "initial";
const PDF_READER_META_KEY_PAGE: &str = "page";

/// Reader for PDF source
///
/// Only a simple version currently.
/// Only able to read text content that are not "drawn".
struct PdfReader {
    source: Document,

    total_pages: usize,
    current_page: usize,

    current_content_chars: Vec<char>,
    current_char_index: usize,

    // current_content: String,
    metadata: JsonValue,
}

#[derive(thiserror::Error)]
enum PdfReaderError {
    #[error(transparent)]
    PdfDocError(#[from] lopdf::Error),
}

impl std::fmt::Debug for PdfReaderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        error_chain_fmt(self, f)
    }
}

impl PdfReader {
    /// Create a PdfReader from a source reader (implementing Read)
    ///
    /// # Params
    /// - reader: SourceReader implementing Read + Seek
    /// - initial_meta: (optional) initial metadata as a JSON object
    pub fn try_from_reader(
        reader: impl Read,
        initial_meta: Option<JsonValue>,
    ) -> Result<Self, PdfReaderError> {
        // Reads all the document in memory: necessary for the lopdf logic
        // let mut buf_full_content = Vec::<u8>::new();
        // let read_len = reader.read_to_end(&mut buf_full_content);
        let source = Document::load_from(reader)?;
        let total_pages = source.get_pages().len();

        let initial_meta = initial_meta.unwrap_or(JsonValue::Null);
        let metadata = match initial_meta {
            JsonValue::Object(map) => json!(map),
            JsonValue::Null => JsonValue::Null,
            _ => json!({ PDF_READER_META_KEY_DEFAULT_INITIAL: initial_meta }),
        };

        info!(
            "PDF reader source: nb pages: {}, initial metadata: {}",
            total_pages, metadata
        );

        Ok(Self {
            source,
            total_pages,
            current_page: 0,
            current_content_chars: vec![],
            current_char_index: 0,
            metadata,
        })
    }

    /// Gets content page by page
    ///
    /// Read the full current PDF page in a String. Not optimal. But act as buffer.
    ///
    /// # Returns
    /// The number of bytes read. 0 if no more content is available.
    fn go_next_content(&mut self) -> Result<usize, PdfReaderError> {
        self.current_char_index = 0;
        let mut content_len = 0;

        // Continues until finding a page that does not have an empty content
        while self.current_page < self.total_pages && content_len == 0 {
            self.current_page += 1;
            let content = self.source.extract_text(&[self.current_page as u32])?;

            content_len = content.len();
            if content_len == 0 {
                continue;
            }

            // UTF-8 is encoded on 1 to 4 bytes. Or only handle unicode scalar values (with char)
            // Unicode scalar values can be more than 1 byte
            self.current_content_chars = content.chars().collect();
            self.update_metadata(PDF_READER_META_KEY_PAGE, json!(self.current_page));
        }

        Ok(content_len)
    }

    /// Updates metadata as a JSON object
    fn update_metadata(&mut self, key: &str, value: JsonValue) {
        if let Some(map) = self.metadata.as_object_mut() {
            map.insert(key.to_owned(), value);
        } else {
            let mut map = Map::new();
            map.insert(key.to_owned(), value);
            self.metadata = JsonValue::Object(map);
        }
    }
}

impl Read for PdfReader {
    // Reads bytes as unicode scalar values
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        // There is no more chars to read from the current content,
        // tries to get next content available from the PDF
        if self.current_char_index >= self.current_content_chars.len() {
            let content_len = self.go_next_content().map_err(|e| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("Error while caching next PDF content: {}", e),
                )
            })?;

            // No more to read
            if content_len == 0 {
                return Ok(0);
            }
        }

        // Fills up the read buffer from the current content
        let mut i = 0;
        // A buffer of length 4 is large enough to encode any `char`
        let mut utf8_char_buf = [0; 4];

        // Tries to fill as much as possible the buffer
        while i < buf.len() && self.current_char_index < self.current_content_chars.len() {
            let current_str_u8 =
                self.current_content_chars[self.current_char_index].encode_utf8(&mut utf8_char_buf);
            let bytes_len = current_str_u8.len();

            // buf length needs to be >= 4
            if i + bytes_len > buf.len() {
                // Not enough space in the buffer to fill the current char
                break;
            }

            for utf8_char in utf8_char_buf.iter().take(bytes_len) {
                buf[i] = *utf8_char;
                i += 1;
            }

            // Goes 1 char at a time
            self.current_char_index += 1;
        }

        Ok(i)
    }
}

impl MetaRead for PdfReader {
    fn get_current_metadata(&self) -> JsonValue {
        json!({ PDF_READER_META_KEY: self.metadata.clone() })
    }
}

#[cfg(test)]
mod tests {
    use std::io::BufReader;

    use super::*;

    #[test]
    fn on_a_correct_3_pages_pdf_it_should_read_content_in_order() {
        let file_name = "sample_3_pages.pdf";
        let file = std::fs::File::open(format!("tests/resources/{file_name}")).unwrap();
        let file_reader = BufReader::new(file);

        let mut pdf_reader =
            PdfReader::try_from_reader(file_reader, Some(json!({ "pdf_name": file_name })))
                .unwrap();

        let mut current_page = 0;
        // `loop` and not `for` loop to test that the read stops by itself when no more content is available
        loop {
            // Buffer big enough to contains content of each page
            let mut buf = [0; 1000];

            match pdf_reader.read(&mut buf) {
                Ok(filling_len) => {
                    if filling_len == 0 {
                        break;
                    }
                    current_page += 1;

                    let read_content = String::from_utf8(buf[0..filling_len].to_vec()).unwrap();

                    // Asserts content
                    assert_eq!(
                        read_content,
                        format!("{current_page}-1: Lorem ipsum \n{current_page}-2: Lorem ipsum \n")
                    );
                    // Asserts metadata
                    assert_eq!(
                        pdf_reader.get_current_metadata()["pdf"]["page"],
                        json!(current_page)
                    );

                    println!(
                        "⛏️ Meta: {}, read content: {}",
                        pdf_reader.get_current_metadata(),
                        read_content
                    );
                }
                Err(error) => {
                    panic!("An error occurred: {:?}", error);
                }
            };
        }

        // Read 3 pages
        assert_eq!(current_page, 3);
    }
}
