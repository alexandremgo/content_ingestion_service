use genawaiter::{rc::gen, yield_, Generator};
use quick_xml::{events::Event, reader::Reader};
use std::{io::BufRead, pin::Pin};
use tracing::debug;

pub const DEFAULT_NB_WORDS_PER_YIELD: usize = 100;

pub const UNWANTED_CHARS: [char; 1] = ['\n'];
const SPECIAL_CHARS_FOR_COUNTING_WORDS: [char; 6] = [',', '.', ';', ':', '?', '!'];

#[derive(Debug)]
enum CharState {
    None,
    Space,
    SpecialForCountingWords(char),
    Normal(char),
}

/// Extracts an content from a buffer, yielding the currently extracted content at every given number of words
///
/// # Arguments
/// * `buf_reader`: buffer for which the content is read from
/// BufReader provides buffering capabilities: tt reads data from an underlying reader in larger chunks to reduce
/// the number of read calls made to the underlying reader, which can improve performance.
/// * `nb_words_per_yield`: limit number of words triggering a new yield. Default to `DEFAULT_NB_WORDS_PER_YIELD`.
///
/// # Returns
/// A generator that yields the content of the epub progressively.
/// The generator is wrapped in a Pin<Box<...>> because, like Future, a Generator can hold a reference into another field of
/// the same struct (becoming a self-referential type). If the Generator is moved, then the reference is incorrect.
/// Pinning the generator to a particular spot in memory prevents this problem, making it safe to create references
/// to values inside the generator block.
#[tracing::instrument(name = "Extracting content using XML parsing", skip(buf_reader))]
pub fn extract_epub_content<'box_lt, BufReaderType: BufRead + 'box_lt>(
    buf_reader: BufReaderType,
    nb_words_per_yield: Option<usize>,
) -> Pin<Box<dyn Generator<Yield = String, Return = Result<(), ()>> + 'box_lt>> {
    let nb_words_per_yield = nb_words_per_yield.unwrap_or(DEFAULT_NB_WORDS_PER_YIELD);
    let mut reader = Reader::from_reader(buf_reader);

    let mut inside_body = 0;
    let mut buf: Vec<u8> = Vec::new();
    let mut current_extracted_content = String::new();
    let mut current_nb_words = 0;
    let mut previous_char_state = CharState::None;

    let generator = gen!({
        // The `Reader` does not implement `Iterator` because it outputs borrowed data (`Cow`s)
        loop {
            match reader.read_event_into(&mut buf) {
                Err(e) => panic!("Error at position {}: {:?}", reader.buffer_position(), e),
                // exits the loop when reaching end of file
                Ok(Event::Eof) => break,

                Ok(Event::Start(e)) => match e.name().as_ref() {
                    b"body" => {
                        debug!("Found <body>");
                        inside_body += 1;
                    }
                    _ => (),
                },
                Ok(Event::End(e)) => match e.name().as_ref() {
                    b"body" => inside_body -= 1,
                    _ => {
                        // On tag closing: always add a space, if there was no space just before.
                        // As previous_char_state is updated, the added space will correctly be processed
                        // at the next event.
                        if inside_body > 0 && !matches!(previous_char_state, CharState::Space) {
                            if matches!(previous_char_state, CharState::Normal(_)) {
                                current_nb_words += 1;
                            }
                            current_extracted_content.push_str(" ");
                            previous_char_state = CharState::Space;
                        }
                    }
                },
                Ok(Event::Text(e)) => {
                    if inside_body > 0 {
                        for element in e.iter() {
                            // Works for utf-8. Need to check for other encoding.
                            let current_char = char::from(element.to_owned());

                            // Trims any unwanted chars
                            if UNWANTED_CHARS.contains(&current_char) {
                                continue;
                            }

                            let current_char_state = if current_char == ' ' {
                                CharState::Space
                            } else if (SPECIAL_CHARS_FOR_COUNTING_WORDS.contains(&current_char)) {
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
                                    "🔍 Reached nb_words_per_yield current document: {:?}",
                                    current_extracted_content
                                );

                                if let CharState::Space = current_char_state {
                                    current_extracted_content.pop();
                                }

                                yield_!(current_extracted_content);

                                // The value is moved in yield_! above, it can't be clear() to keep the memory capacity of the vector/String
                                // Anyway, as the limit is based on the number of words and not the number of chars, keeping the memory
                                // capacity would not work perfectly.
                                // It needs to be cleared with a new String.
                                current_extracted_content = String::new();

                                current_nb_words = 0;
                                previous_char_state = CharState::None;
                            } else {
                                previous_char_state = current_char_state;
                            }
                        }
                    }
                }

                // There are several other `Event`s we do not consider here
                _ => (),
            }
            // if we don't keep a borrow elsewhere, we can clear the buffer to keep memory usage low
            buf.clear();
        }

        debug!("🔍 Last current document: {:?}", current_extracted_content);

        if let CharState::Space = previous_char_state {
            current_extracted_content.pop();
        }
        yield_!(current_extracted_content);

        Ok(())
    });

    // Allocates the generator to the heap so it can be returned as a trait object,
    // and pin the generator to a particular spot in the heap memory.
    // The signature of rc::Generator resume is:
    // fn resume(self: Pin<&mut Self>) -> GeneratorState<Self::Yield, Self::Return>
    Box::pin(generator)
}

#[cfg(test)]
mod test_extract_epub_content {
    use super::*;
    use genawaiter::GeneratorState;
    use std::{io::BufReader, sync::Mutex};

    #[test]
    fn on_empty_input_it_should_extract_empty_content() {
        let content = "";
        let buf_reader = BufReader::new(content.as_bytes());
        let mut generator = extract_epub_content(buf_reader, None);

        // Checks empty yield
        let extracted_content = match generator.as_mut().resume() {
            GeneratorState::Yielded(content) => content,
            _ => panic!("Unexpected generator state"),
        };
        assert_eq!(extracted_content, "");

        // Checks complete
        let extracted_result = match generator.as_mut().resume() {
            GeneratorState::Complete(result) => result,
            _ => panic!("Unexpected generator state"),
        };
        assert_eq!(extracted_result, Ok(()));
    }

    // When the number of words in the EPUB content is < to nb_words_per_yield (only 1 yield)
    #[test]
    fn on_simple_correct_epub_content_it_should_extract_in_1_yield_and_complete() {
        let content = "<html><head><title>Test</title></head><body><p>Test</p></body></html>";
        let buf_reader = BufReader::new(content.as_bytes());
        let mut generator = extract_epub_content(buf_reader, None);

        // Checks 1 yield
        let extracted_content = match generator.as_mut().resume() {
            GeneratorState::Yielded(content) => content,
            _ => panic!("Unexpected generator state"),
        };
        assert_eq!(extracted_content, "Test");

        // Checks complete
        let extracted_result = match generator.as_mut().resume() {
            GeneratorState::Complete(result) => result,
            _ => panic!("Unexpected generator state"),
        };
        assert_eq!(extracted_result, Ok(()));
    }

    #[test]
    fn on_multiline_correct_epub_content_it_should_extract_in_1_yield() {
        let content = "\
    <html>
    <head><title>Test</title></head>
    <body>
        <p>Test</p>
    </body>
    </html>";
        let buf_reader = BufReader::new(content.as_bytes());

        let mut generator = extract_epub_content(buf_reader, None);
        let extracted_content = match generator.as_mut().resume() {
            GeneratorState::Yielded(content) => content,
            _ => panic!("Unexpected generator state"),
        };

        assert_eq!(extracted_content, "Test");
    }

    // On a content with an XML element not representing an actual XML element",
    #[test]
    fn on_xml_element_content_it_should_extract_in_1_yield() {
        // Pay attention to the double spaces
        let content = "<body><p>Test &lt;ok&gt;</p></body>";
        let buf_reader = BufReader::new(content.as_bytes());
        let mut generator = extract_epub_content(buf_reader, None);

        let extracted_content = match generator.as_mut().resume() {
            GeneratorState::Yielded(content) => content,
            _ => panic!("Unexpected generator state"),
        };
        assert_eq!(extracted_content, "Test &lt;ok&gt;");
    }

    #[test]
    fn on_more_complex_contents_is_should_extract_in_1_yield() {
        for i in 1..3 {
            let file =
                std::fs::File::open(format!("tests/resources/simple_{i}_with_result.txt")).unwrap();
            let file_reader = BufReader::new(file);

            // Gets the content to test and the result
            let mut lines_iter = file_reader.lines();
            let content = lines_iter.next().unwrap().unwrap();
            lines_iter.next();
            let result = lines_iter.next().unwrap().unwrap();

            let buf_reader = BufReader::new(content.as_bytes());
            let mut generator = extract_epub_content(buf_reader, None);

            let extracted_content = match generator.as_mut().resume() {
                GeneratorState::Yielded(extracted_content) => extracted_content,
                _ => panic!("Unexpected generator state"),
            };

            assert_eq!(extracted_content, result);
        }
    }

    // When the number of words in the EPUB content is > to nb_words_per_yield (several yields)
    // On a simple and correct EPUB content that is: nb_words_per_yield < length < 2 * nb_words_per_yield",
    #[test]
    fn on_bigger_content_it_should_extract_in_2_yields() {
        let content = "<html><head><title>Non-extracted title</title></head><body><p>Test</p>Ok - how are you?</body></html>";
        let result_1 = "Test Ok - how";
        let result_2 = "are you?";

        let buf_reader = BufReader::new(content.as_bytes());
        let mut generator = extract_epub_content(buf_reader, Some(4));

        let extracted_content = match generator.as_mut().resume() {
            GeneratorState::Yielded(content) => content,
            _ => panic!("Unexpected generator state"),
        };
        assert_eq!(extracted_content, result_1);

        let extracted_content = match generator.as_mut().resume() {
            GeneratorState::Yielded(content) => content,
            _ => panic!("Unexpected generator state"),
        };
        assert_eq!(extracted_content, result_2);

        // Completes
        let extracted_result = match generator.as_mut().resume() {
            GeneratorState::Complete(result) => result,
            _ => panic!("Unexpected generator state"),
        };
        assert_eq!(extracted_result, Ok(()));
    }

    // When the number of words in the EPUB content is > to nb_words_per_yield (several yields)
    // On a more complex and correct EPUB content with: (x - 1) * nb_words_per_yield < number of words < x * nb_words_per_yield",
    #[test]
    fn on_much_bigger_complex_content_it_should_extract_in_several_yields() {
        // Arranges
        let expected_yielded_contents = vec![
            "It is nice to finally meet you.",
            "Would you like some coffee? I love",
            "coffee. I drink it every morning.",
            "!!!!!!!!",
            "Could you please pass me the sugar?",
        ];
        let expected_nb_yields = expected_yielded_contents.len();
        let content = format!(
            "<html><head><title>Test</title></head><body><p>{}</p></body></html>",
            expected_yielded_contents.join(" ")
        );

        let buf_reader = BufReader::new(content.as_bytes());
        let mut generator = extract_epub_content(buf_reader, Some(8));

        // Asserts each yield
        for i in 0..expected_nb_yields {
            let yielded_extracted_content = match generator.as_mut().resume() {
                GeneratorState::Yielded(content) => content,
                _ => panic!("Unexpected generator state"),
            };
            assert_eq!(yielded_extracted_content, expected_yielded_contents[i]);
        }
    }
}
