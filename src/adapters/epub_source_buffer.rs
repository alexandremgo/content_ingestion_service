use epub::doc::EpubDoc;
use std::{fs::File, io::BufReader};

use crate::domain::entities::source::SourceChar;
use crate::ports::source_buffer_port::{NextError, SourceBufferPort};

pub struct EpubSourceBuffer {
    source: EpubDoc<BufReader<File>>,
    current_content_chars: Vec<char>,
    current_content_chars_index: usize,
    previous_content_id: String,
    // Tried to keep alive an iterator on the current content, but difficulty with lifetime
    // current_content_chars: Option<Box<Chars<'content_lt>>>,
    // current_content: String, // Needed so the current content lives long enough so the reference cur_cotnent_chars can exist
}

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

impl EpubSourceBuffer {
    pub fn try_new<'content_lt>(source_file_path: String) -> Result<Self, TryNewError> {
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

        let (current_content, cur_mime) = match source.get_current_str() {
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

        Ok(EpubSourceBuffer {
            source,
            current_content_chars,
            current_content_chars_index: 0,
            previous_content_id: String::from(""), // TODO: empty string = bad ?
        })
    }

    fn next_content(&mut self) -> Result<(), NextContentError> {
        self.source.go_next();

        let (current_content, _cur_mime) = match self.source.get_current_str() {
            None => {
                return Err(NextContentError::Ended);
            }
            Some(result) => result,
        };

        // To avoid infinitely looping on the last content
        let current_content_id = self.source.get_current_id().unwrap();
        if current_content_id.eq(&self.previous_content_id) {
            return Err(NextContentError::Ended);
        }

        self.current_content_chars = current_content.chars().collect();
        self.current_content_chars_index = 0;
        self.previous_content_id = current_content_id;
        // self.current_content_chars = Some(self.current_content.chars());

        Ok(())
    }
}

impl SourceBufferPort for EpubSourceBuffer {
    fn next(&mut self) -> Result<Option<SourceChar>, NextError> {
        self.current_content_chars_index += 1;

        match self.current_content_chars.get(self.current_content_chars_index) {
            None => {
                match self.next_content() {
                    Ok(_) => self.next(),
                    Err(e) => {
                        match e {
                            NextContentError::Ended => Ok(None)
                        }
                    }
                }
            }
            Some(c) => Ok(Some(SourceChar {
                value: c.clone(),
                page: 0,
            })),
        }
    }
}


#[cfg(test)]
mod tests {
    extern crate speculate;
    use speculate::speculate;

    use super::*;

    speculate! {
        describe "epub_source_buffer" {
            it "should work" {
                let mut source_buffer = EpubSourceBuffer::try_new(String::from("src/tests/accessible_epub_3.epub")).unwrap();
                // let mut source_buffer = EpubSourceBuffer::try_new(String::from("src/tests/minimal_sample.epub")).unwrap();

                println!("🔮🔮🔮🔮🔮🔮🔮 Let's go");

                // TODO: will need another watchdog so we don't run infinitely if there is a loop
                // for _i in 0..1000000 {
                loop {
                    let c = match source_buffer.next() {
                        Ok(result) => match result {
                            None => break,
                            Some(source_char) => source_char
                        },
                        Err(error) => {
                            panic!("An error occurred: {:?}", error);
                        }
                    };
                    print!("{}", c.value);
                }

                println!("🔮🔮🔮🔮🔮🔮🔮 THE END");
                assert_eq!(1, 2);
            }
        }
    }
}
