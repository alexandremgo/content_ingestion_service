use epub::doc::{DocError, EpubDoc};
use serde_json::{json, Map, Value as JsonValue};
use std::io::{Read, Seek};
use tracing::{debug, info};

use crate::helper::error_chain_fmt;

use super::meta_read::MetaRead;

const EPUB_READER_META_KEY: &str = "epub";
const EPUB_READER_META_KEY_DEFAULT_INITIAL: &str = "initial";

/// EPUB reader
///
/// An EPUB is an archive file consisting of XHTML files carrying the content, along with images and other supporting file.
/// This reader read the content of the EPUB following its linear reading order defined in its `spine` element.
///
/// The read content contains XHTML. It needs to be read/wrapped with an XML reader.
///
/// [Seek](https://doc.rust-lang.org/stable/std/io/trait.Seek.html) implementation is needed
///
/// As we cannot get a reference to the inner reader of `EpubDoc`, we cannot currently compose with a `SourceReader` implementing `MetaRead`
pub struct EpubReader<SourceReader: Read + Seek> {
    source: EpubDoc<SourceReader>,

    // Avoids looping in EpubDoc reader
    previous_content_id: String,

    current_content_chars: Vec<char>,
    current_char_index: usize,

    // MetaRead
    metadata: JsonValue,

    // Flag to avoid infinite read loop
    is_read_completed: bool,
}

#[derive(thiserror::Error)]
pub enum EpubReaderError {
    #[error(transparent)]
    EpubDocError(#[from] DocError),
}

impl std::fmt::Debug for EpubReaderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        error_chain_fmt(self, f)
    }
}

#[derive(Debug)]
pub enum NextContentError {
    Ended,
}

impl<SourceReader: Read + Seek> EpubReader<SourceReader> {
    /// Create an EpubReader from a source reader (implementing Read + Seek)
    ///
    /// # Params
    /// - reader: SourceReader implementing Read + Seek
    /// - initial_meta: (optional) initial metadata as a JSON object
    #[tracing::instrument(name = "Creating EPUB reader", skip(reader))]
    pub fn from_reader(
        reader: SourceReader,
        initial_meta: Option<JsonValue>,
    ) -> Result<Self, EpubReaderError> {
        let source = EpubDoc::from_reader(reader)?;

        let initial_meta = initial_meta.unwrap_or(JsonValue::Null);
        let metadata = match initial_meta {
            JsonValue::Object(map) => json!(map),
            JsonValue::Null => JsonValue::Null,
            _ => json!({ EPUB_READER_META_KEY_DEFAULT_INITIAL: initial_meta }),
        };

        info!("Reader initial metadata: {}", metadata);

        Ok(EpubReader {
            source,
            previous_content_id: String::from(""),
            current_content_chars: vec![],
            current_char_index: 0,
            metadata,
            is_read_completed: false,
        })
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

    /// Gets content chapter by chapter
    ///
    /// Read the full current EPUB chapter in a String. Not optimal. But act as a (big) buffer.
    ///
    /// # Returns
    /// The number of bytes read. 0 if no more content is available.
    fn go_next_content(&mut self) -> Result<usize, EpubReaderError> {
        let mut content_len = 0;
        self.current_char_index = 0;

        while content_len == 0 {
            self.source.go_next();

            // `source.get_current_path` and then try to read each file with a Reader ?
            let (current_content, _cur_mime) = match self.source.get_current_str() {
                None => {
                    // No more thing to read
                    self.is_read_completed = true;
                    return Ok(0);
                }
                Some(result) => result,
            };

            // To avoid infinitely looping on the last content
            let current_content_id = self.source.get_current_id().unwrap_or("".to_string());

            if current_content_id.eq(&self.previous_content_id) {
                debug!("Encountered the same content id: {}", current_content_id);
                self.is_read_completed = true;
                return Ok(0);
            }

            self.previous_content_id = current_content_id;

            content_len = current_content.len();
            if content_len == 0 {
                continue;
            }

            // To improve
            self.update_metadata(
                "chapter_path",
                json!(self
                    .source
                    .get_current_path()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string()),
            );
            self.update_metadata("chapter_number", json!(self.source.get_current_page()));
            self.update_metadata("chapters_size", json!(self.source.get_num_pages()));
            self.update_metadata("chapter_id", json!(self.source.get_current_id()));

            // UTF-8 is encoded on 1 to 4 bytes. Or only handle unicode scalar values (with char)
            // Unicode scalar values can be more than 1 byte
            self.current_content_chars = current_content.chars().collect();
        }

        Ok(content_len)
    }
}

impl<SourceReader: Read + Seek> Read for EpubReader<SourceReader> {
    // Version with Unicode scalar values
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.is_read_completed {
            return Ok(0);
        }

        // There is no more chars to read from the current content,
        // tries to get next content available from EPUB
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

            for j in 0..bytes_len {
                buf[i] = utf8_char_buf[j];
                i += 1;
            }

            // Goes 1 char at a time
            self.current_char_index += 1;
        }

        Ok(i)
    }
}

impl<SourceReader: Read + Seek> MetaRead for EpubReader<SourceReader> {
    fn get_current_metadata(&self) -> JsonValue {
        json!({ EPUB_READER_META_KEY: self.metadata.clone() })
    }
}

#[cfg(test)]
mod epub_reader_tests {
    use std::io::BufReader;

    use super::*;

    #[test]
    fn on_correct_epub_it_creates_a_content_reader() {
        let file =
            std::fs::File::open(String::from("tests/resources/accessible_epub_3.epub")).unwrap();
        let file_reader = BufReader::new(file);

        let mut source_buffer = EpubReader::from_reader(
            file_reader,
            Some(json!({ "book_title": "accessible_epub_3" })),
        )
        .unwrap();

        // let mut source_buffer = EpubReader::try_new(String::from("src/tests/minimal_sample.epub")).unwrap();

        println!("EPUB READER 🔮🔮🔮🔮🔮🔮🔮 Let's go");

        // TODO: will need another watchdog so we don't run infinitely if there is a loop
        // for _i in 0..1000000 {
        loop {
            let mut buf = [0; 8000];
            match source_buffer.read(&mut buf) {
                Ok(filling_len) => {
                    println!("Filled with {} bytes", filling_len);
                    if filling_len == 0 {
                        println!("NO MORE TO READ");
                        break;
                    }
                    println!(
                        "Content: meta: {}\n {}\n-----\n",
                        source_buffer.get_current_metadata(),
                        // filling_len,
                        String::from_utf8(buf[0..filling_len].to_vec()).unwrap()
                    );
                }
                Err(error) => {
                    panic!("An error occurred: {:?}", error);
                }
            };
        }

        println!("🔮🔮🔮🔮🔮🔮🔮 THE END");
        assert_eq!(1, 1);
    }
}
