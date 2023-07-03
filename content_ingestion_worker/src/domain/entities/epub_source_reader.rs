use epub::doc::EpubDoc;
use std::cmp::min;
use std::io::Read;
use std::{fs::File, io::BufReader};
use tracing::info;

// use crate::domain::entities::source::SourceChar;
// use crate::ports::source_buffer_port::{NextError, SourceBufferPort};

pub struct EpubSourceReader {
    source: EpubDoc<BufReader<File>>,
    // current_content_chars: Vec<char>,
    current_content_chars_index: usize,
    previous_content_id: String,

    current_content_bytes: Vec<u8>, // Box<[u8]>
    current_byte_index: usize,
    current_content_chars: Vec<char>,
    current_char_index: usize,
    // Tried to keep alive an iterator on the current content, but difficulty with lifetime
    // current_content_chars: Option<Box<Chars<'content_lt>>>,
    // current_content: String, // Needed so the current content lives long enough so the reference cur_cotnent_chars can exist
}

// TODO: EpubSourceReaderError
#[derive(Debug)]
pub enum TryNewError {
    ArchiveError(String),
    XmlError(String),
    IOError(String),
    InvalidEpub(String),
    NoContent(String),
}

#[derive(Debug)]
pub enum NextContentError {
    Ended,
}

impl EpubSourceReader {
    pub fn try_new(source_file_path: String) -> Result<Self, TryNewError> {
        let source = EpubDoc::new(source_file_path);
        let mut source = match source {
            Ok(source) => source,
            Err(e) => {
                let try_new_error = match e {
                    epub::doc::DocError::ArchiveError(epub_error) => {
                        TryNewError::ArchiveError(format!("{:?}", epub_error))
                    }
                    epub::doc::DocError::XmlError(epub_error) => {
                        TryNewError::XmlError(format!("{:?}", epub_error))
                    }
                    epub::doc::DocError::IOError(epub_error) => {
                        TryNewError::IOError(format!("{:?}", epub_error))
                    }
                    epub::doc::DocError::InvalidEpub => {
                        TryNewError::InvalidEpub(String::from("Unknown error"))
                    }
                };

                return Err(try_new_error);
            }
        };

        let (current_content, _cur_mime) = match source.get_current_str() {
            None => {
                return Err(TryNewError::NoContent(String::from(
                    "No content was found in the EPUB",
                )));
            }
            Some(content) => content,
        };

        // Is there a way to keep alive only an iterator ?
        // For doing that, it seemed like copying the current_content was needed, so not an improvement
        let current_content_chars = current_content.chars().collect();

        Ok(EpubSourceReader {
            source,
            current_content_chars,
            current_content_chars_index: 0,
            previous_content_id: String::from(""), // TODO: empty string = bad ?

            current_byte_index: 0,
            current_content_bytes: vec![],
            current_char_index: 0,
        })
    }
}

impl Read for EpubSourceReader {
    // Version with Unicode scalar values
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        // There is no more chars to read from the current content,
        // tries to get next content available from EPUB
        if self.current_char_index >= self.current_content_chars.len() {
            self.source.go_next();

            // Read the full current EPUB chapter in a String. Not optimal. But act as a (big) buffer.
            // `source.get_current_path` and then try to read each file with a Reader ?
            let (current_content, _cur_mime) = match self.source.get_current_str() {
                None => {
                    // No more thing to read
                    return Ok(0);
                }
                Some(result) => result,
            };

            // To avoid infinitely looping on the last content
            let current_content_id = self.source.get_current_id().unwrap();
            if current_content_id.eq(&self.previous_content_id) {
                return Ok(0);
            }

            self.previous_content_id = current_content_id;
            // TODO: here actually read chars ? or graphene ?
            // UTF-8 is encoded on 1 to 4 bytes. Or only handle unicode scalar values (with char)
            // Unicode scalar values can be more than 1 byte
            self.current_content_chars = current_content.chars().collect();
            self.current_char_index = 0;
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

    // Version with bytes
    // fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
    //     // There is no more bytes to read from the current content,
    //     // tries to get next content available from EPUB
    //     if self.current_byte_index >= self.current_content_bytes.len() {
    //         println!("🐊 No more bytes to read from the current content, tries to get next content available from EPUB");
    //         self.source.go_next();

    //         // Read the full current EPUB chapter in a String. Not optimal. But act as a (big) buffer.
    //         // `source.get_current_path` and then try to read each file with a Reader ?
    //         let (current_content, _cur_mime) = match self.source.get_current_str() {
    //             None => {
    //                 // No more thing to read
    //                 return Ok(0);
    //             }
    //             Some(result) => result,
    //         };

    //         // To avoid infinitely looping on the last content
    //         let current_content_id = self.source.get_current_id().unwrap();
    //         if current_content_id.eq(&self.previous_content_id) {
    //             return Ok(0);
    //         }

    //         self.previous_content_id = current_content_id;
    //         self.current_byte_index = 0;
    //         // TODO: here actually read chars ? or graphene ?
    //         // UTF-8 is encoded on 1 to 4 bytes. Or only handle unicode scalar values (with char)
    //         self.current_content_bytes = current_content.as_bytes().to_vec();
    //     }

    //     // Fills up the read buffer from the current content
    //     let remaining_content_bytes_len =
    //         self.current_content_bytes.len() - self.current_byte_index;

    //     let buf_len = buf.len();
    //     let filling_len = min(buf_len, remaining_content_bytes_len);
    //     info!(
    //         "🐊 Read buf len: {} | remaining content bytes len: {} | filling length: {}",
    //         buf_len, remaining_content_bytes_len, filling_len
    //     );

    //     for i in 0..filling_len {
    //         buf[i] = self.current_content_bytes[i + self.current_byte_index];
    //     }
    //     self.current_byte_index = self.current_byte_index + filling_len;

    //     Ok(filling_len)
    // }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn on_correct_epub_it_creates_a_content_reader() {
        let mut source_buffer =
            EpubSourceReader::try_new(String::from("tests/resources/accessible_epub_3.epub"))
                .unwrap();
        // let mut source_buffer = EpubSourceReader::try_new(String::from("src/tests/minimal_sample.epub")).unwrap();

        println!("🔮🔮🔮🔮🔮🔮🔮 Let's go");

        // TODO: will need another watchdog so we don't run infinitely if there is a loop
        // for _i in 0..1000000 {
        loop {
            let mut buf = [0; 200];
            match source_buffer.read(&mut buf) {
                Ok(filling_len) => {
                    println!("Filled with {} bytes", filling_len);
                    if filling_len == 0 {
                        println!("NO MORE TO READ");
                        break;
                    }
                    println!(
                        "Content: {}\n\n",
                        String::from_utf8(buf[0..filling_len].to_vec()).unwrap()
                    );
                }
                Err(error) => {
                    panic!("An error occurred: {:?}", error);
                }
            };
        }

        println!("🔮🔮🔮🔮🔮🔮🔮 THE END");
        assert_eq!(1, 2);
    }
}
