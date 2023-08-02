use futures::Future;
use genawaiter::{
    sync::{gen, Gen},
    yield_,
};
use serde_json::Value as JsonValue;
use std::{io::Read, pin::Pin};
use tracing::{debug, error};

use crate::{
    domain::entities::{extracted_content::ExtractedContent, meta_read::MetaRead},
    helper::error_chain_fmt,
};

pub const DEFAULT_NB_WORDS_PER_YIELD: usize = 100;

const SPECIAL_CHARS_FOR_COUNTING_WORDS: [char; 6] = [',', '.', ';', ':', '?', '!'];

#[derive(Debug)]
enum CharState {
    None,
    Space,
    SpecialForCountingWords(char),
    Normal(char),
}

#[derive(thiserror::Error)]
pub enum ExtractContentGeneratorError {
    #[error(transparent)]
    ReadError(#[from] std::io::Error),
    #[error(transparent)]
    Utf8Error(#[from] std::string::FromUtf8Error),
}

impl std::fmt::Debug for ExtractContentGeneratorError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        error_chain_fmt(self, f)
    }
}

/// Extracts contents from a reader
///
/// An "extracted content" is defined as a content limited by a certain number of words and metadata associated to it.
/// See `ExtractedContent` struct.
///
/// # Arguments
/// * `reader`: reader from which the content is read from
/// * `nb_words_per_yield`: limit number of words triggering a new yield (length of an extracted content).
///      Default to `DEFAULT_NB_WORDS_PER_YIELD`.
///
/// # Returns
/// A generator that progressively yields `ExtractedContent`s read from the reader.
/// Using the `genawaiter::sync` implementation which allocates and can be shared between threads.
/// The generator is wrapped in a Pin<Box<...>> because, like Future, a Generator can hold a reference into another field of
/// the same struct (becoming a self-referential type). If the Generator is moved, then the reference is incorrect.
/// Pinning the generator to a particular spot in memory prevents this problem, making it safe to create references
/// to values inside the generator block.
#[tracing::instrument(name = "Extracting contents from a reader", skip(reader))]
pub fn extract_content_generator<'box_lt, ReaderType: Read + MetaRead + 'box_lt>(
    reader: &'box_lt mut ReaderType,
    nb_words_per_yield: Option<usize>,
) -> Pin<
    Box<
        Gen<
            ExtractedContent,
            (),
            impl Future<Output = Result<(), ExtractContentGeneratorError>> + 'box_lt,
        >,
    >,
> {
    let nb_words_per_yield = nb_words_per_yield.unwrap_or(DEFAULT_NB_WORDS_PER_YIELD);
    let mut previous_metadata = JsonValue::Null;
    let mut current_extracted_content = String::new();
    let mut current_nb_words = 0;
    let mut previous_char_state = CharState::None;
    // Arbitrary size for the buffer, could be fine tuned
    let mut buf = [0; 1000];

    let generator = gen!({
        loop {
            match reader.read(&mut buf) {
                Ok(read_len) => {
                    // Nothing to read anymore
                    if read_len == 0 {
                        break;
                    }

                    let metadata = reader.get_current_metadata();

                    // Separates extracted content by meta
                    if metadata != previous_metadata {
                        debug!(
                            "Metadata changed: previous: {} | new: {}",
                            previous_metadata, metadata
                        );

                        if current_nb_words > 0 {
                            yield_!(ExtractedContent::new(
                                current_extracted_content,
                                previous_metadata.clone()
                            ));

                            // Resets
                            current_nb_words = 0;
                            current_extracted_content = String::new();
                            previous_char_state = CharState::None;
                        }

                        previous_metadata = metadata;
                    }

                    let read_content = String::from_utf8(buf[0..read_len].to_vec())?;
                    for current_char in read_content.chars() {
                        // Trims any unwanted chars
                        if UNWANTED_CHARS.contains(&current_char) {
                            continue;
                        }

                        let current_char_state = if current_char == ' ' {
                            CharState::Space
                        } else if SPECIAL_CHARS_FOR_COUNTING_WORDS.contains(&current_char) {
                            CharState::SpecialForCountingWords(current_char.to_owned())
                        } else {
                            CharState::Normal(current_char.to_owned())
                        };

                        // Counts a word after it ends (by a space or a special-for-counting-words char)
                        // A special-for-counting-words char always increment the counter by 1
                        match previous_char_state {
                            CharState::None => {
                                match current_char_state {
                                    CharState::Space => {
                                        // Jumps to the next char if spaces while the content is empty
                                        continue;
                                    }
                                    CharState::SpecialForCountingWords(_) => {
                                        current_nb_words += 1;
                                    }
                                    _ => (),
                                }
                            }
                            CharState::Space => {
                                match current_char_state {
                                    CharState::Space => {
                                        // Jumps to the next char if successive spaces
                                        continue;
                                    }
                                    CharState::SpecialForCountingWords(_) => {
                                        current_nb_words += 1;
                                    }
                                    _ => (),
                                }
                            }
                            CharState::SpecialForCountingWords(_) => match current_char_state {
                                CharState::SpecialForCountingWords(_) => {
                                    current_nb_words += 1;
                                }
                                _ => (),
                            },
                            CharState::Normal(_) => match current_char_state {
                                CharState::Space => {
                                    current_nb_words += 1;
                                }
                                CharState::SpecialForCountingWords(_) => {
                                    // 1 for the word that ended, 1 for the special-for-counting-words char
                                    current_nb_words += 2;
                                }
                                _ => (),
                            },
                        };

                        current_extracted_content.push(current_char);

                        if current_nb_words >= nb_words_per_yield {
                            debug!(
                                "Reached nb_words_per_yield current extracted content: {}",
                                current_nb_words
                            );

                            yield_!(ExtractedContent::new(
                                current_extracted_content,
                                previous_metadata.clone()
                            ));

                            // Resets
                            current_nb_words = 0;

                            // The value is moved in yield_! above, it can't be clear() to keep the memory capacity of the vector/String
                            // Anyway, as the limit is based on the number of words and not the number of chars, keeping the memory
                            // capacity would not work perfectly.
                            // It needs to be cleared with a new String.
                            current_extracted_content = String::new();
                            previous_char_state = CharState::None;
                        } else {
                            previous_char_state = current_char_state;
                        }
                    }
                }
                Err(error) => {
                    error!("Error while extracting content: {}", error);
                    return Err(error.into());
                }
            };
        }

        // Yields last extracted content
        if let CharState::Space = previous_char_state {
            current_extracted_content.pop();
        }

        yield_!(ExtractedContent::new(
            current_extracted_content,
            previous_metadata
        ));

        Ok(())
    });

    // Allocates the generator to the heap so it can be returned as a trait object,
    // and pin the generator to a particular spot in the heap memory.
    // The signature of rc::Generator resume is:
    // fn resume(self: Pin<&mut Self>) -> GeneratorState<Self::Yield, Self::Return>
    // TODO: using sync::Generator now
    Box::pin(generator)
}

#[cfg(test)]
mod tests {
    use crate::domain::entities::simple_metadata_reader::{
        SimpleMetadataReader, SIMPLE_READER_META_KEY,
    };

    use super::*;
    use genawaiter::GeneratorState;
    use serde_json::json;
    use std::io::BufReader;

    #[test]
    fn on_empty_source_reader_it_should_extract_empty_content() {
        let content = "";
        let buf_reader = BufReader::new(content.as_bytes());
        let mut simple_reader = SimpleMetadataReader::new(buf_reader, None);
        let mut generator = extract_content_generator(&mut simple_reader, Some(100));

        // Checks empty yield
        let extracted_content = match generator.as_mut().resume() {
            GeneratorState::Yielded(content) => content,
            _ => panic!("Unexpected generator state"),
        };
        assert_eq!(extracted_content.content, "");

        // Checks complete
        let extracted_result = match generator.as_mut().resume() {
            GeneratorState::Complete(result) => result,
            _ => panic!("Unexpected generator state"),
        };
        assert!(matches!(extracted_result, Ok(())));
    }

    #[test]
    fn on_source_fitting_in_1_yield_it_should_extract_in_1_yield_and_complete() {
        let content = "Test some 1 yield text";
        let buf_reader = BufReader::new(content.as_bytes());
        let mut simple_reader = SimpleMetadataReader::new(buf_reader, None);
        let mut generator = extract_content_generator(&mut simple_reader, Some(100));

        // Checks 1 yield
        let extracted_content = match generator.as_mut().resume() {
            GeneratorState::Yielded(content) => content,
            _ => panic!("Unexpected generator state"),
        };
        assert_eq!(extracted_content.content, "Test some 1 yield text");

        // Checks complete
        let extracted_result = match generator.as_mut().resume() {
            GeneratorState::Complete(result) => result,
            _ => panic!("Unexpected generator state"),
        };
        assert!(matches!(extracted_result, Ok(())));
    }

    #[test]
    fn on_source_fitting_in_x_yields_it_should_extract_in_x_yields_and_complete() {
        // Arranges: 8 words per sentence, except the last sentence.
        let expected_yielded_contents = vec![
            "It is nice to finally meet you.",
            "Would you like some coffee? I love",
            "coffee. I drink it every morning.",
            "!!!!!!!!",
            "Do you want sugar?",
        ];
        let content = expected_yielded_contents.join(" ");

        let buf_reader = BufReader::new(content.as_bytes());
        let mut simple_reader = SimpleMetadataReader::new(buf_reader, None);
        let mut generator = extract_content_generator(&mut simple_reader, Some(8));

        // Asserts each yield
        for expected_content in expected_yielded_contents {
            let yielded_extracted_content = match generator.as_mut().resume() {
                GeneratorState::Yielded(content) => content,
                _ => panic!("Unexpected generator state"),
            };
            assert_eq!(yielded_extracted_content.content.trim(), expected_content);
        }

        // Checks complete
        let extracted_result = match generator.as_mut().resume() {
            GeneratorState::Complete(result) => result,
            _ => panic!("Unexpected generator state"),
        };
        assert!(matches!(extracted_result, Ok(())));
    }

    #[test]
    fn on_source_with_metadata_it_should_extract_content_with_metadata() {
        let content = "Test some 1 yield text";
        let source_metadata_key = "source_key";
        let source_metadata_value = "some value";

        let buf_reader = BufReader::new(content.as_bytes());
        let mut simple_reader = SimpleMetadataReader::new(
            buf_reader,
            Some(json!({ source_metadata_key: source_metadata_value })),
        );
        let mut generator = extract_content_generator(&mut simple_reader, Some(100));

        // Checks 1 yield
        let extracted_content = match generator.as_mut().resume() {
            GeneratorState::Yielded(content) => content,
            _ => panic!("Unexpected generator state"),
        };

        assert_eq!(
            extracted_content.metadata[SIMPLE_READER_META_KEY][source_metadata_key],
            json!(source_metadata_value)
        );
    }
}

/// 65 control codes characters
pub const UNWANTED_CHARS: [char; 65] = [
    '\u{0000}', // Null
    '\u{0001}', // Start of Heading
    '\u{0002}', // Start of Text
    '\u{0003}', // End of Text
    '\u{0004}', // End of Transmission
    '\u{0005}', // Enquiry
    '\u{0006}', // Acknowledge
    '\u{0007}', // Bell
    '\u{0008}', // Backspace
    '\u{0009}', // Horizontal Tab
    '\u{000A}', // Line Feed
    '\u{000B}', // Vertical Tab
    '\u{000C}', // Form Feed
    '\u{000D}', // Carriage Return
    '\u{000E}', // Shift Out
    '\u{000F}', // Shift In
    '\u{0010}', // Data Link Escape
    '\u{0011}', // Device Control 1 (XON)
    '\u{0012}', // Device Control 2
    '\u{0013}', // Device Control 3 (XOFF)
    '\u{0014}', // Device Control 4
    '\u{0015}', // Negative Acknowledge
    '\u{0016}', // Synchronous Idle
    '\u{0017}', // End of Transmission Block
    '\u{0018}', // Cancel
    '\u{0019}', // End of Medium
    '\u{001A}', // Substitute
    '\u{001B}', // Escape
    '\u{001C}', // File Separator
    '\u{001D}', // Group Separator
    '\u{001E}', // Record Separator
    '\u{001F}', // Unit Separator
    '\u{007F}', // Delete
    '\u{0080}', // PADDING CHARACTER
    '\u{0081}', // HIGH OCTET PRESET
    '\u{0082}', // BREAK PERMITTED HERE
    '\u{0083}', // NO BREAK HERE
    '\u{0084}', // INDEX
    '\u{0085}', // NEXT LINE
    '\u{0086}', // START OF SELECTED AREA
    '\u{0087}', // END OF SELECTED AREA
    '\u{0088}', // CHARACTER TABULATION SET
    '\u{0089}', // CHARACTER TABULATION WITH JUSTIFICATION
    '\u{008A}', // LINE TABULATION SET
    '\u{008B}', // PARTIAL LINE FORWARD
    '\u{008C}', // PARTIAL LINE BACKWARD
    '\u{008D}', // REVERSE LINE FEED
    '\u{008E}', // SINGLE SHIFT TWO
    '\u{008F}', // SINGLE SHIFT THREE
    '\u{0090}', // DEVICE CONTROL STRING
    '\u{0091}', // PRIVATE USE ONE
    '\u{0092}', // PRIVATE USE TWO
    '\u{0093}', // SET TRANSMIT STATE
    '\u{0094}', // CANCEL CHARACTER
    '\u{0095}', // MESSAGE WAITING
    '\u{0096}', // START OF GUARDED AREA
    '\u{0097}', // END OF GUARDED AREA
    '\u{0098}', // START OF STRING
    '\u{0099}', // SINGLE CHARACTER INTRODUCER
    '\u{009A}', // CONTROL SEQUENCE INTRODUCER
    '\u{009B}', // STRING TERMINATOR
    '\u{009C}', // STRING TERMINATOR
    '\u{009D}', // OPERATING SYSTEM COMMAND
    '\u{009E}', // PRIVACY MESSAGE
    '\u{009F}', // APPLICATION PROGRAM COMMAND
];
