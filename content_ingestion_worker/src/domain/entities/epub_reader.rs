use epub::doc::{DocError, EpubDoc};
use std::io::{Read, Seek};
use tracing::debug;

use crate::helper::error_chain_fmt;

use super::meta_read::MetaRead;

// use crate::domain::entities::source::SourceChar;
// use crate::ports::source_buffer_port::{NextError, SourceBufferPort};

/// [Seek](https://doc.rust-lang.org/stable/std/io/trait.Seek.html) implementation is needed
/// To be composable: SourceReader: Read + Seek + MetaRead too
pub struct EpubReader<SourceReader: Read + Seek> {
    source: EpubDoc<SourceReader>,

    // Avoids looping in EpubDoc reader
    previous_content_id: String,

    current_content_bytes: Vec<u8>, // Box<[u8]>
    current_byte_index: usize,
    current_content_chars: Vec<char>,
    current_char_index: usize,
    // Tried to keep alive an iterator on the current content, but difficulty with lifetime
    // current_content_chars: Option<Box<Chars<'content_lt>>>,
    // current_content: String, // Needed so the current content lives long enough so the reference cur_cotnent_chars can exist

    // MetaRead
    current_meta: Option<String>,
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
    pub fn from_reader(reader: SourceReader) -> Result<Self, EpubReaderError> {
        let source = EpubDoc::from_reader(reader)?;

        Ok(EpubReader {
            source,
            previous_content_id: String::from(""),
            current_byte_index: 0,
            current_content_bytes: vec![],
            current_content_chars: vec![],
            current_char_index: 0,

            current_meta: None,
        })
    }
}

impl<SourceReader: Read + Seek> Read for EpubReader<SourceReader> {
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
            let current_content_id = self.source.get_current_id().unwrap_or("".to_string());
            if current_content_id.eq(&self.previous_content_id) {
                debug!("Encountered the same content id: {}", current_content_id);
                return Ok(0);
            }

            // To improve
            self.current_meta = Some(format!(
                "Chapter: {:?}",
                self.source.get_current_path().unwrap_or_default()
            ));

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

impl<SourceReader: Read + Seek> MetaRead for EpubReader<SourceReader> {
    fn current_read_meta(&self) -> Option<String> {
        self.current_meta.clone()
    }
}

// /// Implements BufRead on EpubReader
// impl<SourceReader: Read + Seek> BufRead for EpubReader<SourceReader> {
//     fn fill_buf(&mut self) -> std::io::Result<&[u8]> {
//         let result = self.current_content_chars.iter().map(|c| *c as u8).collect::<Vec<_>>().as_slice();
//         Ok(result)
//     }

//     fn consume(&mut self, amt: usize) {
//         // `fill_buf` never returns an error
//         let mut full_buf = self.fill_buf().unwrap();
//         let new_buf = full_buf[amt..].to_vec(); // .iter().map()
//         let buf_str = from_utf8(&new_buf);
//     }
// }

#[cfg(test)]
mod tests {
    use std::io::BufReader;

    use super::*;

    #[test]
    fn on_correct_epub_it_creates_a_content_reader() {
        let file =
            std::fs::File::open(String::from("tests/resources/accessible_epub_3.epub")).unwrap();
        let file_reader = BufReader::new(file);

        let mut source_buffer = EpubReader::from_reader(file_reader).unwrap();

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
                        "Content: meta: {:?}\n {}\n-----\n",
                        source_buffer.current_read_meta(),
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
        assert_eq!(1, 2);
    }
}
