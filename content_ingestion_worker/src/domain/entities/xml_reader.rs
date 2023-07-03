use quick_xml::events::Event;
use std::io::{BufRead, BufReader, ErrorKind, Read};
use tracing::debug;

use crate::helper::error_chain_fmt;

use super::meta_read::MetaRead;

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

pub struct XMLReader<SourceReader: Read + MetaRead> {
    /// XML inner reader, wrapping any `BufReader`
    /// `BufRead` implementation is needed for `read_event_into`
    /// `BufReader` is needed to access to the inner reader (via `get_ref` for ex)
    reader: quick_xml::reader::Reader<BufReader<SourceReader>>,

    current_content_chars: Vec<char>,
    current_char_index: usize,
    current_content_inside_body: usize,

    // MetaRead
    current_meta: Option<String>,
}

/// Builds a new XMLReader from a reader not implementing BufRead
pub fn build_from_reader<SourceReader: Read + MetaRead>(
    reader: SourceReader,
) -> XMLReader<SourceReader> {
    // `BufRead` implementation is needed for `read_event_into`
    let buf_reader = BufReader::new(reader);
    let reader = quick_xml::reader::Reader::from_reader(buf_reader);

    XMLReader {
        reader,
        current_meta: None,
        current_content_chars: vec![],
        current_char_index: 0,
        current_content_inside_body: 0,
    }
}

// /// Builds a new XMLReader from a reader implementing BufRead
// pub fn build_from_buf_reader<R: BufRead>(buf_reader: R) -> XMLReader<R> {
//     let reader = quick_xml::reader::Reader::from_reader(buf_reader);

//     XMLReader {
//         reader,
//         current_meta: None,
//         current_content_chars: vec![],
//         current_char_index: 0,
//         current_content_inside_body: 0,
//     }
// }

impl<SourceReader: Read + MetaRead> XMLReader<SourceReader> {
    /// Caches the content appearing inside the next XML tags
    #[tracing::instrument(name = "Caching next XML content", skip(self))]
    fn next_content(&mut self) -> Result<(), XMLReaderError> {
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
                        self.current_content_inside_body += 1;
                    }
                    name => {
                        // TODO: JSON (and we don't care about the current tags)
                        // And it needs to access the source reader meta
                        self.current_meta =
                            Some(format!("current tag: {:?}", std::str::from_utf8(name)));
                    }
                },
                Ok(Event::End(e)) => match e.name().as_ref() {
                    b"body" => self.current_content_inside_body -= 1,
                    _ => {
                        // On tag closing: always add a space, if there was no space just before.
                        if self.current_content_inside_body > 0 {
                            self.current_content_chars.push(' ');
                        }
                    }
                },
                Ok(Event::Text(e)) => {
                    if self.current_content_inside_body > 0 {
                        let next_content: Vec<char> = e
                            .iter()
                            .map(|element| char::from(element.to_owned()))
                            .collect();

                        // Filters out empty content (happening after a tag is closed)
                        if next_content.len() == 0
                            || next_content.len() < 2 && next_content[0] == ' '
                        {
                            continue;
                        }

                        // Stops once a content inside <body> is read
                        self.current_content_chars.extend(next_content);
                        break;
                    }
                }

                // There are several other `Event`s we do not consider here
                _ => (),
            }
            // If we don't keep a borrow elsewhere, we can clear the buffer to keep memory usage low
            buf.clear();
        }

        Ok(())
    }
}

impl<SourceReader: Read + MetaRead> Read for XMLReader<SourceReader> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        // There is no more chars to read from the current content,
        // tries to get next content available from EPUB
        if self.current_char_index >= self.current_content_chars.len() {
            self.next_content().map_err(|err| {
                std::io::Error::new(
                    ErrorKind::InvalidData,
                    format!("Error while caching next XML content: {}", err),
                )
            })?;
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

/// Gets the meta information of the currently read chunk
///
/// Adds the current XMLReader meta information to the current meta information of the wrapped source reader
impl<SourceReader: Read + MetaRead> MetaRead for XMLReader<SourceReader> {
    fn current_read_meta(&self) -> Option<String> {
        Some(format!(
            "{:?} -> {:?}",
            self.reader.get_ref().get_ref().current_read_meta(),
            self.current_meta.clone()
        ))
    }
}

#[cfg(test)]
mod test_xml_reader {
    use super::*;
    use fake::{faker::lorem::en::Sentences, Fake};

    #[test]
    fn on_empty_input_it_should_read_an_empty_content() {
        let content = "";
        let source_reader = DumbMetaReader::new(content.as_bytes());
        let mut xml_reader = build_from_reader(source_reader);

        let mut extracted_content = String::new();

        loop {
            let mut buf = [0; 100];
            match xml_reader.read(&mut buf) {
                Ok(filling_len) => {
                    if filling_len == 0 {
                        break;
                    }
                    let read_content = String::from_utf8(buf[0..filling_len].to_vec()).unwrap();
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
        let source_reader = DumbMetaReader::new(content.as_bytes());
        let mut xml_reader = build_from_reader(source_reader);

        let mut extracted_content = String::new();
        loop {
            let mut buf = [0; 100];
            match xml_reader.read(&mut buf) {
                Ok(filling_len) => {
                    if filling_len == 0 {
                        break;
                    }
                    let read_content = String::from_utf8(buf[0..filling_len].to_vec()).unwrap();
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
        let source_reader = DumbMetaReader::new(content.as_bytes());
        let mut xml_reader = build_from_reader(source_reader);

        let mut extracted_content = String::new();
        loop {
            let mut buf = [0; 100];
            match xml_reader.read(&mut buf) {
                Ok(filling_len) => {
                    if filling_len == 0 {
                        break;
                    }
                    let read_content = String::from_utf8(buf[0..filling_len].to_vec()).unwrap();
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
                return format!("<p>{}</p>", sentence);
            })
            .collect();
        let content = format!(
            "<html><head><title>Test</title></head><body>{}</body></html>",
            tagged_content.join("")
        );

        let source_reader = DumbMetaReader::new(content.as_bytes());
        let mut xml_reader = build_from_reader(source_reader);

        // Acts
        let mut extracted_content = String::new();
        loop {
            // The buffer is big enough to receive each sentence
            let mut buf = [0; 1000];
            match xml_reader.read(&mut buf) {
                Ok(filling_len) => {
                    if filling_len == 0 {
                        break;
                    }
                    let read_content = String::from_utf8(buf[0..filling_len].to_vec()).unwrap();
                    // TODO: test on meta
                    println!(
                        "Read: meta = {}\ncontent={}\n\n",
                        xml_reader.current_read_meta().unwrap(),
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

        let source_reader = DumbMetaReader::new(content.as_bytes());
        let mut xml_reader = build_from_reader(source_reader);

        // Acts
        let mut extracted_content = String::new();
        loop {
            // The buffer is too small to receive the 1st and 3th sentence
            let mut buf = [0; 10];
            match xml_reader.read(&mut buf) {
                Ok(filling_len) => {
                    if filling_len == 0 {
                        break;
                    }
                    let read_content = String::from_utf8(buf[0..filling_len].to_vec()).unwrap();
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

        let source_reader = DumbMetaReader::new(content.as_bytes());
        let mut xml_reader = build_from_reader(source_reader);

        // Acts
        let mut extracted_content = String::new();
        loop {
            // The buffer is big enough to receive each sentence
            let mut buf = [0; 1000];
            match xml_reader.read(&mut buf) {
                Ok(filling_len) => {
                    if filling_len == 0 {
                        break;
                    }
                    let read_content = String::from_utf8(buf[0..filling_len].to_vec()).unwrap();
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

        let source_reader = DumbMetaReader::new(content.as_bytes());
        let mut xml_reader = build_from_reader(source_reader);

        // Acts
        let mut extracted_content = String::new();
        let mut received_error: Option<std::io::Error> = None;
        loop {
            // The buffer is big enough to receive each sentence
            let mut buf = [0; 1000];
            match xml_reader.read(&mut buf) {
                Ok(filling_len) => {
                    if filling_len == 0 {
                        break;
                    }
                    let read_content = String::from_utf8(buf[0..filling_len].to_vec()).unwrap();
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

    // Help struct to have a reader implementing MetaRead
    struct DumbMetaReader<Reader: Read> {
        reader: Reader,
    }

    impl<Reader: Read> MetaRead for DumbMetaReader<Reader> {
        fn current_read_meta(&self) -> Option<String> {
            Some("XMLReader fake meta".to_string())
        }
    }

    impl<Reader: Read> DumbMetaReader<Reader> {
        pub fn new(reader: Reader) -> Self {
            Self { reader }
        }
    }

    impl<Reader: Read> Read for DumbMetaReader<Reader> {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            self.reader.read(buf)
        }
    }
}
