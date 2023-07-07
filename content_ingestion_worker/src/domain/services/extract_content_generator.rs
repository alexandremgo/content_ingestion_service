use genawaiter::{rc::gen, yield_, Generator};
use serde_json::Value as JsonValue;
use std::{io::Read, pin::Pin};
use tracing::{debug, error};

use crate::domain::entities::meta_read::MetaRead;

pub const DEFAULT_NB_WORDS_PER_YIELD: usize = 100;

const SPECIAL_CHARS_FOR_COUNTING_WORDS: [char; 6] = [',', '.', ';', ':', '?', '!'];

#[derive(Debug)]
enum CharState {
    None,
    Space,
    SpecialForCountingWords(char),
    Normal(char),
}

pub struct Document {
    pub meta: JsonValue,
    pub content: String,
}

/// Extracts documents from a reader
///
/// A document is defined as a content limited by a certain number of words and metadata associated to it.
/// See `Document` struct.
///
/// # Arguments
/// * `reader`: reader from which the content is read from
/// * `nb_words_per_yield`: limit number of words triggering a new yield (length of a document).
///      Default to `DEFAULT_NB_WORDS_PER_YIELD`.
///
/// # Returns
/// A generator that progressively yields `Document`s read from the reader.
/// The generator is wrapped in a Pin<Box<...>> because, like Future, a Generator can hold a reference into another field of
/// the same struct (becoming a self-referential type). If the Generator is moved, then the reference is incorrect.
/// Pinning the generator to a particular spot in memory prevents this problem, making it safe to create references
/// to values inside the generator block.
///
/// TODO: why not Read for the type ? Because of xml Reader !
#[tracing::instrument(name = "Extracting documents from a reader", skip(reader))]
pub fn extract_content_generator<'box_lt, ReaderType: Read + MetaRead + 'box_lt>(
    reader: &'box_lt mut ReaderType,
    nb_words_per_yield: Option<usize>,
) -> Pin<Box<dyn Generator<Yield = Document, Return = Result<(), ()>> + 'box_lt>> {
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

                    // Separates document by meta
                    if metadata != previous_metadata {
                        debug!(
                            "⛏️ Meta changed: previous meta: {} | new meta: {}",
                            previous_metadata, metadata
                        );

                        if current_nb_words > 0 {
                            debug!("⛏️ Meta changed, yielding with {} words", current_nb_words);

                            yield_!(Document {
                                content: current_extracted_content,
                                meta: previous_metadata.clone()
                            });

                            // Resets
                            current_nb_words = 0;
                            current_extracted_content = String::new();
                            previous_char_state = CharState::None;
                        }

                        previous_metadata = metadata;
                    }

                    // TODO: unwrapping: error or continue
                    let read_content = String::from_utf8(buf[0..read_len].to_vec()).unwrap();
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
                                "⛏️ Reached nb_words_per_yield current document: {}",
                                current_nb_words
                            );

                            yield_!(Document {
                                content: current_extracted_content,
                                meta: previous_metadata.clone()
                            });

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

                    // extracted_content.push_str(&read_content);
                }
                Err(error) => {
                    error!("⛏️ Error while extracting content: {}", error);
                    // TODO: return error ?
                    break;
                }
            };
        }

        // Yields last extracted content
        if let CharState::Space = previous_char_state {
            current_extracted_content.pop();
        }

        yield_!(Document {
            content: current_extracted_content,
            meta: previous_metadata
        });

        Ok(())
    });

    // Allocates the generator to the heap so it can be returned as a trait object,
    // and pin the generator to a particular spot in the heap memory.
    // The signature of rc::Generator resume is:
    // fn resume(self: Pin<&mut Self>) -> GeneratorState<Self::Yield, Self::Return>
    Box::pin(generator)
}

// #[cfg(test)]
// mod test_extract_content_generator {
//     use super::*;
//     use genawaiter::GeneratorState;
//     use std::{io::BufReader, sync::Mutex};

//     #[test]
//     fn on_empty_input_it_should_extract_empty_content() {
//         let content = "";
//         let buf_reader = BufReader::new(content.as_bytes());
//         let mut generator = extract_content_generator(buf_reader, None);

//         // Checks empty yield
//         let extracted_content = match generator.as_mut().resume() {
//             GeneratorState::Yielded(content) => content,
//             _ => panic!("Unexpected generator state"),
//         };
//         assert_eq!(extracted_content, "");

//         // Checks complete
//         let extracted_result = match generator.as_mut().resume() {
//             GeneratorState::Complete(result) => result,
//             _ => panic!("Unexpected generator state"),
//         };
//         assert_eq!(extracted_result, Ok(()));
//     }

//     // When the number of words in the EPUB content is < to nb_words_per_yield (only 1 yield)
//     #[test]
//     fn on_simple_correct_epub_content_it_should_extract_in_1_yield_and_complete() {
//         let content = "<html><head><title>Test</title></head><body><p>Test</p></body></html>";
//         let buf_reader = BufReader::new(content.as_bytes());
//         let mut generator = extract_content_generator(buf_reader, None);

//         // Checks 1 yield
//         let extracted_content = match generator.as_mut().resume() {
//             GeneratorState::Yielded(content) => content,
//             _ => panic!("Unexpected generator state"),
//         };
//         assert_eq!(extracted_content, "Test");

//         // Checks complete
//         let extracted_result = match generator.as_mut().resume() {
//             GeneratorState::Complete(result) => result,
//             _ => panic!("Unexpected generator state"),
//         };
//         assert_eq!(extracted_result, Ok(()));
//     }

//     #[test]
//     fn on_multiline_correct_epub_content_it_should_extract_in_1_yield() {
//         let content = "\
//     <html>
//     <head><title>Test</title></head>
//     <body>
//         <p>Test</p>
//     </body>
//     </html>";
//         let buf_reader = BufReader::new(content.as_bytes());

//         let mut generator = extract_content_generator(buf_reader, None);
//         let extracted_content = match generator.as_mut().resume() {
//             GeneratorState::Yielded(content) => content,
//             _ => panic!("Unexpected generator state"),
//         };

//         assert_eq!(extracted_content, "Test");
//     }

//     // On a content with an XML element not representing an actual XML element",
//     #[test]
//     fn on_xml_element_content_it_should_extract_in_1_yield() {
//         // Pay attention to the double spaces
//         let content = "<body><p>Test &lt;ok&gt;</p></body>";
//         let buf_reader = BufReader::new(content.as_bytes());
//         let mut generator = extract_content_generator(buf_reader, None);

//         let extracted_content = match generator.as_mut().resume() {
//             GeneratorState::Yielded(content) => content,
//             _ => panic!("Unexpected generator state"),
//         };
//         assert_eq!(extracted_content, "Test &lt;ok&gt;");
//     }

//     #[test]
//     fn on_more_complex_contents_is_should_extract_in_1_yield() {
//         for i in 1..3 {
//             let file =
//                 std::fs::File::open(format!("tests/resources/simple_{i}_with_result.txt")).unwrap();
//             let file_reader = BufReader::new(file);

//             // Gets the content to test and the result
//             let mut lines_iter = file_reader.lines();
//             let content = lines_iter.next().unwrap().unwrap();
//             lines_iter.next();
//             let result = lines_iter.next().unwrap().unwrap();

//             let buf_reader = BufReader::new(content.as_bytes());
//             let mut generator = extract_content_generator(buf_reader, None);

//             let extracted_content = match generator.as_mut().resume() {
//                 GeneratorState::Yielded(extracted_content) => extracted_content,
//                 _ => panic!("Unexpected generator state"),
//             };

//             assert_eq!(extracted_content, result);
//         }
//     }

//     // When the number of words in the EPUB content is > to nb_words_per_yield (several yields)
//     // On a simple and correct EPUB content that is: nb_words_per_yield < length < 2 * nb_words_per_yield",
//     #[test]
//     fn on_bigger_content_it_should_extract_in_2_yields() {
//         let content = "<html><head><title>Non-extracted title</title></head><body><p>Test</p>Ok - how are you?</body></html>";
//         let result_1 = "Test Ok - how";
//         let result_2 = "are you?";

//         let buf_reader = BufReader::new(content.as_bytes());
//         let mut generator = extract_content_generator(buf_reader, Some(4));

//         let extracted_content = match generator.as_mut().resume() {
//             GeneratorState::Yielded(content) => content,
//             _ => panic!("Unexpected generator state"),
//         };
//         assert_eq!(extracted_content, result_1);

//         let extracted_content = match generator.as_mut().resume() {
//             GeneratorState::Yielded(content) => content,
//             _ => panic!("Unexpected generator state"),
//         };
//         assert_eq!(extracted_content, result_2);

//         // Completes
//         let extracted_result = match generator.as_mut().resume() {
//             GeneratorState::Complete(result) => result,
//             _ => panic!("Unexpected generator state"),
//         };
//         assert_eq!(extracted_result, Ok(()));
//     }

//     // When the number of words in the EPUB content is > to nb_words_per_yield (several yields)
//     // On a more complex and correct EPUB content with: (x - 1) * nb_words_per_yield < number of words < x * nb_words_per_yield",
//     #[test]
//     fn on_much_bigger_complex_content_it_should_extract_in_several_yields() {
//         // Arranges
//         let expected_yielded_contents = vec![
//             "It is nice to finally meet you.",
//             "Would you like some coffee? I love",
//             "coffee. I drink it every morning.",
//             "!!!!!!!!",
//             "Could you please pass me the sugar?",
//         ];
//         let expected_nb_yields = expected_yielded_contents.len();
//         let content = format!(
//             "<html><head><title>Test</title></head><body><p>{}</p></body></html>",
//             expected_yielded_contents.join(" ")
//         );

//         let buf_reader = BufReader::new(content.as_bytes());
//         let mut generator = extract_content_generator(buf_reader, Some(8));

//         // Asserts each yield
//         for i in 0..expected_nb_yields {
//             let yielded_extracted_content = match generator.as_mut().resume() {
//                 GeneratorState::Yielded(content) => content,
//                 _ => panic!("Unexpected generator state"),
//             };
//             assert_eq!(yielded_extracted_content, expected_yielded_contents[i]);
//         }
//     }
// }

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
