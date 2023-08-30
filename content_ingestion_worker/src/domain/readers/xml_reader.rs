use common::helper::error_chain_fmt;
use quick_xml::events::Event;
use serde_json::{json, Map, Value as JsonValue};
use std::io::{BufReader, ErrorKind, Read};
use tracing::debug;

use crate::domain::entities::meta_read::MetaRead;

#[derive(thiserror::Error)]
pub enum XMLReaderError {
    #[error("{0}")]
    NextContentError(String),
}

impl std::fmt::Debug for XMLReaderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        error_chain_fmt(self, f)
    }
}

const XML_READER_META_KEY: &str = "xml";
const XML_READER_META_KEY_TITLE: &str = "title";

/// XML reader
///
/// Currently supports EPUB/HTML like XML syntax.
///
/// Would need to be more generic if the source does not use tags like <body> and <title>
pub struct XMLReader<SourceReader: Read + MetaRead> {
    /// XML inner reader, wrapping any `BufReader`
    /// `BufRead` implementation is needed for `read_event_into`
    /// `BufReader` is needed to access to the inner reader (via `get_ref` for ex)
    reader: quick_xml::reader::Reader<BufReader<SourceReader>>,

    current_content_chars: Vec<char>,
    current_char_index: usize,
    current_inside_body: usize,
    current_inside_title: usize,

    // MetaRead
    metadata: JsonValue,
}

/// Builds a new XMLReader from a reader not implementing BufRead
#[tracing::instrument(name = "Creating XML reader", skip(reader))]
pub fn build_from_reader<SourceReader: Read + MetaRead>(
    reader: SourceReader,
) -> XMLReader<SourceReader> {
    // `BufRead` implementation is needed for `read_event_into`
    let buf_reader = BufReader::new(reader);
    let reader = quick_xml::reader::Reader::from_reader(buf_reader);

    XMLReader {
        reader,
        metadata: JsonValue::Null,
        current_content_chars: vec![],
        current_char_index: 0,
        current_inside_body: 0,
        current_inside_title: 0,
    }
}

// /// Builds a new XMLReader from a reader implementing BufRead
// pub fn build_from_buf_reader<R: BufRead>(buf_reader: R) -> XMLReader<R> {
//     let reader = quick_xml::reader::Reader::from_reader(buf_reader);

//     XMLReader {
//         reader,
//         metadata: None,
//         current_content_chars: vec![],
//         current_char_index: 0,
//         current_inside_body: 0,
//     }
// }

impl<SourceReader: Read + MetaRead> XMLReader<SourceReader> {
    /// Caches the content appearing inside the next XML tags
    /// # Returns
    /// The number of char read. 0 if no more content is available.
    fn go_next_content(&mut self) -> Result<usize, XMLReaderError> {
        let mut buf: Vec<u8> = Vec::new();

        // Re-initializes current content variables
        self.current_content_chars = vec![];
        self.current_char_index = 0;

        // The `Reader` does not implement `Iterator` because it outputs borrowed data (`Cow`s)
        loop {
            match self.reader.read_event_into(&mut buf) {
                Err(e) => {
                    return Err(XMLReaderError::NextContentError(format!(
                        "Error at position {}: {:?}",
                        self.reader.buffer_position(),
                        e
                    )));
                }
                // Exits the loop when reaching end of file
                Ok(Event::Eof) => break,
                Ok(Event::Start(e)) => match e.name().as_ref() {
                    b"body" => {
                        debug!("Found <body>");
                        self.current_inside_body += 1;
                    }
                    b"title" => {
                        debug!("Found <title>");
                        self.current_inside_title += 1;
                    }
                    _name => {
                        // Idea: having a list of tags that define separate documents (like a new <h1>)
                        // self.update_metadata(
                        //     "tag",
                        //     json!(std::str::from_utf8(name).unwrap_or_default()),
                        // );
                    }
                },
                Ok(Event::End(e)) => match e.name().as_ref() {
                    b"body" => self.current_inside_body -= 1,
                    b"title" => self.current_inside_title -= 1,
                    _ => {
                        // On tag closing: always add a space, if there was no space just before.
                        if self.current_inside_body > 0 {
                            self.current_content_chars.push(' ');
                        }
                    }
                },
                Ok(Event::Text(e)) => {
                    if self.current_inside_body > 0 {
                        let next_content: Vec<char> = e
                            .iter()
                            .map(|element| char::from(element.to_owned()))
                            .collect();

                        if next_content.is_empty() {
                            debug!("Content length = 0");
                            return Ok(0);
                        }

                        // Filters out 1-space-content (happening after a tag is closed)
                        if next_content.len() == 1 && next_content[0] == ' ' {
                            continue;
                        }

                        // Stops once a content inside <body> is read
                        self.current_content_chars.extend(next_content);
                        break;
                    }
                    // Normally we can't be inside a <title> and <body>
                    else if self.current_inside_title > 0 {
                        let title = e.unescape().unwrap_or_default().to_string();
                        self.update_metadata(XML_READER_META_KEY_TITLE, json!(title));
                    }
                }

                // There are several other `Event`s we do not consider here
                _ => (),
            }
            // If we don't keep a borrow elsewhere, we can clear the buffer to keep memory usage low
            buf.clear();
        }

        Ok(self.current_content_chars.len())
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

impl<SourceReader: Read + MetaRead> Read for XMLReader<SourceReader> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        // There is no more chars to read from the current content,
        // tries to get next content available from EPUB
        if self.current_char_index >= self.current_content_chars.len() {
            let read_chars_len = self.go_next_content().map_err(|err| {
                std::io::Error::new(
                    ErrorKind::InvalidData,
                    format!("Error while caching next XML content: {}", err),
                )
            })?;

            if read_chars_len == 0 {
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

/// Gets the metadata of the currently read chunk
///
/// Adds the current XMLReader metadata to the current metadata of the wrapped source reader
impl<SourceReader: Read + MetaRead> MetaRead for XMLReader<SourceReader> {
    fn get_current_metadata(&self) -> JsonValue {
        let mut source_meta = self.reader.get_ref().get_ref().get_current_metadata();

        if let Some(map) = source_meta.as_object_mut() {
            map.insert(XML_READER_META_KEY.to_string(), self.metadata.clone());
            source_meta
        } else {
            let mut map = Map::new();
            map.insert(XML_READER_META_KEY.to_string(), self.metadata.clone());
            JsonValue::Object(map)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::readers::simple_metadata_reader::{
        SimpleMetadataReader, SIMPLE_READER_META_KEY,
    };
    use fake::{faker::lorem::en::Sentences, Fake};

    // ----- Tests on content reader -----

    #[test]
    fn on_empty_input_it_should_read_an_empty_content() {
        let content = "";
        let source_reader = SimpleMetadataReader::new(content.as_bytes(), None);
        let mut xml_reader = build_from_reader(source_reader);

        let mut extracted_content = String::new();

        loop {
            let mut buf = [0; 100];
            match xml_reader.read(&mut buf) {
                Ok(read_len) => {
                    if read_len == 0 {
                        break;
                    }
                    let read_content = String::from_utf8(buf[0..read_len].to_vec()).unwrap();
                    extracted_content.push_str(&read_content);
                }
                Err(error) => {
                    panic!("An error occurred: {:?}", error);
                }
            };
        }

        assert_eq!(extracted_content, "");
    }

    #[test]
    fn on_simple_xml_content_it_should_read_the_body_content() {
        let content = "<html><head><title>Test</title></head><body><p>Test</p></body></html>";
        let source_reader = SimpleMetadataReader::new(content.as_bytes(), None);
        let mut xml_reader = build_from_reader(source_reader);

        let mut extracted_content = String::new();
        loop {
            let mut buf = [0; 100];
            match xml_reader.read(&mut buf) {
                Ok(read_len) => {
                    if read_len == 0 {
                        break;
                    }
                    let read_content = String::from_utf8(buf[0..read_len].to_vec()).unwrap();
                    extracted_content.push_str(&read_content);
                }
                Err(error) => {
                    panic!("An error occurred: {:?}", error);
                }
            };
        }

        assert_eq!(extracted_content, "Test ");
    }

    #[test]
    fn on_multiline_correct_xml_content_it_should_read_the_body_content() {
        let content = "\
    <html>
    <head><title>Test</title></head>
    <body>
        <p>Test</p>
    </body>
    </html>";
        let source_reader = SimpleMetadataReader::new(content.as_bytes(), None);
        let mut xml_reader = build_from_reader(source_reader);

        let mut extracted_content = String::new();
        loop {
            let mut buf = [0; 100];
            match xml_reader.read(&mut buf) {
                Ok(read_len) => {
                    if read_len == 0 {
                        break;
                    }
                    let read_content = String::from_utf8(buf[0..read_len].to_vec()).unwrap();
                    extracted_content.push_str(&read_content);
                }
                Err(error) => {
                    panic!("An error occurred: {:?}", error);
                }
            };
        }

        assert_eq!(extracted_content, "\n        Test \n    ");
    }

    #[test]
    fn on_several_tagged_sentences_it_should_read_the_content_from_each_tag() {
        // Arranges
        let expected_content: Vec<String> = Sentences(3..10).fake();
        let tagged_content: Vec<String> = expected_content
            .iter()
            .enumerate()
            .map(|(i, sentence)| {
                if i % 2 == 0 {
                    return format!("<h2>{}</h2>", sentence);
                }
                format!("<p>{}</p>", sentence)
            })
            .collect();
        let content = format!(
            "<html><head><title>Test</title></head><body>{}</body></html>",
            tagged_content.join("")
        );

        let source_reader = SimpleMetadataReader::new(content.as_bytes(), None);
        let mut xml_reader = build_from_reader(source_reader);
        // Acts
        let mut extracted_content = String::new();
        loop {
            // The buffer is big enough to receive each sentence
            let mut buf = [0; 1000];
            match xml_reader.read(&mut buf) {
                Ok(read_len) => {
                    if read_len == 0 {
                        break;
                    }
                    let read_content = String::from_utf8(buf[0..read_len].to_vec()).unwrap();

                    println!(
                        "Read: meta = {}\ncontent={}\n\n",
                        xml_reader.get_current_metadata(),
                        read_content
                    );
                    extracted_content.push_str(&read_content);
                }
                Err(error) => {
                    panic!("An error occurred: {:?}", error);
                }
            };
        }

        // Asserts
        assert_eq!(extracted_content.trim(), expected_content.join(" "));
    }

    #[test]
    fn when_using_small_read_buffer_it_should_still_read_correctly_entire_content() {
        // Arranges
        let expected_content = vec![
            "A long sentence that is more than 10 bytes",
            "small",
            "Another long sentence that is more than 10 bytes",
        ];
        let tagged_content: Vec<String> = expected_content
            .iter()
            .map(|sentence| format!("<p>{}</p>", sentence))
            .collect();
        let content = format!(
            "<html><head><title>Test</title></head><body>{}</body></html>",
            tagged_content.join("")
        );

        let source_reader = SimpleMetadataReader::new(content.as_bytes(), None);
        let mut xml_reader = build_from_reader(source_reader);

        // Acts
        let mut extracted_content = String::new();
        loop {
            // The buffer is too small to receive the 1st and 3th sentence
            let mut buf = [0; 10];
            match xml_reader.read(&mut buf) {
                Ok(read_len) => {
                    if read_len == 0 {
                        break;
                    }
                    let read_content = String::from_utf8(buf[0..read_len].to_vec()).unwrap();
                    extracted_content.push_str(&read_content);
                }
                Err(error) => {
                    panic!("An error occurred: {:?}", error);
                }
            };
        }

        // Asserts
        assert_eq!(extracted_content.trim(), expected_content.join(" "));
    }

    #[test]
    fn on_content_without_body_it_should_read_an_empty_content() {
        // Arranges
        let expected_content: Vec<String> = Sentences(3..10).fake();
        let tagged_content: Vec<String> = expected_content
            .iter()
            .map(|sentence| format!("<p>{}</p>", sentence))
            .collect();
        // NO <body></body>
        let content = format!(
            "<html><head><title>Test</title></head>{}</html>",
            tagged_content.join("")
        );

        let source_reader = SimpleMetadataReader::new(content.as_bytes(), None);
        let mut xml_reader = build_from_reader(source_reader);

        // Acts
        let mut extracted_content = String::new();
        loop {
            // The buffer is big enough to receive each sentence
            let mut buf = [0; 1000];
            match xml_reader.read(&mut buf) {
                Ok(read_len) => {
                    if read_len == 0 {
                        break;
                    }
                    let read_content = String::from_utf8(buf[0..read_len].to_vec()).unwrap();
                    extracted_content.push_str(&read_content);
                }
                Err(error) => {
                    panic!("An error occurred: {:?}", error);
                }
            };
        }

        // Asserts
        assert_eq!(extracted_content, "");
    }

    #[test]
    fn on_incorrectly_tagged_content_it_should_read_an_empty_content() {
        // Arranges
        let expected_content: Vec<String> = Sentences(3..10).fake();
        // Closing several times the same tag
        let tagged_content: Vec<String> = expected_content
            .iter()
            .map(|sentence| format!("<p>{}</p></p>", sentence))
            .collect();
        let content = format!(
            "<html><head><title>Test</title></head><body>{}</body></html>",
            tagged_content.join("")
        );

        let source_reader = SimpleMetadataReader::new(content.as_bytes(), None);
        let mut xml_reader = build_from_reader(source_reader);

        // Acts
        let mut extracted_content = String::new();
        let mut received_error: Option<std::io::Error> = None;
        loop {
            // The buffer is big enough to receive each sentence
            let mut buf = [0; 1000];
            match xml_reader.read(&mut buf) {
                Ok(read_len) => {
                    if read_len == 0 {
                        break;
                    }
                    let read_content = String::from_utf8(buf[0..read_len].to_vec()).unwrap();
                    extracted_content.push_str(&read_content);
                }
                Err(error) => {
                    // Expects to receive an error, stops after it
                    received_error = Some(error);
                    break;
                }
            };
        }

        // Asserts
        assert!(matches!(
            received_error.unwrap().kind(),
            std::io::ErrorKind::InvalidData
        ));
    }

    // ----- Tests on metadata -----
    // Only test on `title` meta

    #[test]
    fn on_several_body_and_title_sections_it_should_update_metadata_info_correctly_and_propagate_source_meta(
    ) {
        // Arranges
        let titles: Vec<String> = Sentences(2..3).fake();
        let expected_content: Vec<String> = Sentences(3..10).fake();

        // A different title half the time
        let tagged_content: Vec<String> = expected_content
            .iter()
            .enumerate()
            .map(|(i, sentence)| {
                format!(
                    "<head><title>{}</title></head><body><p>{}</p></body>",
                    titles[i % 2],
                    sentence
                )
            })
            .collect();
        let content = format!("<html>{}</html>", tagged_content.join(""));

        let source_metadata_key = "test_key";
        let source_metadata_value = "a value";

        let source_reader = SimpleMetadataReader::new(
            content.as_bytes(),
            Some(json!({ source_metadata_key: source_metadata_value })),
        );
        let mut xml_reader = build_from_reader(source_reader);

        // Acts
        let mut read_counter = 0;
        loop {
            // The buffer is big enough to receive each sentence
            let mut buf = [0; 1000];
            match xml_reader.read(&mut buf) {
                Ok(read_len) => {
                    if read_len == 0 {
                        break;
                    }
                    let read_content = String::from_utf8(buf[0..read_len].to_vec()).unwrap();

                    // Just to avoid last space on the last read
                    if read_content.len() > 1 {
                        // Asserts on the `title` meta
                        assert_eq!(
                            json!(titles[read_counter % 2]),
                            xml_reader.get_current_metadata()[XML_READER_META_KEY]
                                [XML_READER_META_KEY_TITLE]
                        );

                        // Asserts on the propagated meta (from source reader)
                        assert_eq!(
                            json!(source_metadata_value),
                            xml_reader.get_current_metadata()[SIMPLE_READER_META_KEY]
                                [source_metadata_key]
                        );
                    }

                    read_counter += 1;
                }
                Err(error) => {
                    panic!("An error occurred: {:?}", error);
                }
            };
        }
    }
}
