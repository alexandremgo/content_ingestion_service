use genawaiter::{rc::gen, yield_, Generator};
use quick_xml::{events::Event, reader::Reader};
use std::{
    io::{BufRead, BufReader},
    pin::Pin,
};

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

/// Extracts the content from a buffer, yielding the currently extracted content at every given number of words
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
pub fn xml_extract_content<'box_lt, BufReaderType: BufRead + 'box_lt>(
    buf_reader: BufReaderType,
    nb_words_per_yield: Option<usize>,
) -> Pin<Box<dyn Generator<Yield = String, Return = Result<(), ()>> + 'box_lt>> {
    let nb_words_per_yield = nb_words_per_yield.unwrap_or(DEFAULT_NB_WORDS_PER_YIELD);
    let mut reader = Reader::from_reader(buf_reader);

    println!("🧠 xml_extract_content");

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
                    b"title" => println!("title !"),
                    b"body" => inside_body += 1,
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
                                println!(
                                    "🧠 Reached nb_words_per_yield 🔥 current document: {:?}",
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

        println!("🧠 Last current document: {:?}", current_extracted_content);

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

use demonstrate::demonstrate;

demonstrate! {
    describe "xml_extract_content" {
        use genawaiter::GeneratorState;
        use super::*;

        describe "When the input is empty" {
            it "should extract an empty content and complete" {
                let content = "";
                let buf_reader = BufReader::new(content.as_bytes());
                let mut generator = xml_extract_content(buf_reader, None);
                let extracted_content = match generator.as_mut().resume() {
                    GeneratorState::Yielded(content) => content,
                    _ => panic!("Unexpected generator state"),
                };
                assert_eq!(extracted_content, "");

                // Completes
                let extracted_result = match generator.as_mut().resume() {
                    GeneratorState::Complete(result) => result,
                    _ => panic!("Unexpected generator state"),
                };
                assert_eq!(extracted_result, Ok(()));
            }
        }

        describe "When the number of words in the EPUB content is < to nb_words_per_yield (only 1 yield)" {
            describe "On a simple and correct EPUB content" {
                it "it should extract the content correctly in 1 yield, and complete" {
                    let content = "<html><head><title>Test</title></head><body><p>Test</p></body></html>";
                    let buf_reader = BufReader::new(content.as_bytes());
                    let mut generator = xml_extract_content(buf_reader, None);
                    let extracted_content = match generator.as_mut().resume() {
                        GeneratorState::Yielded(content) => content,
                        _ => panic!("Unexpected generator state"),
                    };
                    assert_eq!(extracted_content, "Test");

                    // Completes
                    let extracted_result = match generator.as_mut().resume() {
                        GeneratorState::Complete(result) => result,
                        _ => panic!("Unexpected generator state"),
                    };
                    assert_eq!(extracted_result, Ok(()));
                }
            }

            describe "On a multiline, with spaces, correct EPUB content" {
                it "it should extract the content correctly in 1 yield" {
                    let content = "\
    <html>
    <head><title>Test</title></head>
    <body>
        <p>Test</p>
    </body>
    </html>";
                    let buf_reader = BufReader::new(content.as_bytes());

                    let mut generator = xml_extract_content(buf_reader, None);
                    let extracted_content = match generator.as_mut().resume() {
                        GeneratorState::Yielded(content) => content,
                        _ => panic!("Unexpected generator state"),
                    };
                    assert_eq!(extracted_content, "Test");
                }
            }

            // TODO: is it possible to convert encoded '&lt;' into '<' ?
            describe "On a content with an XML element not representing an actual XML element" {
                it "it should extract the content correctly in 1 yield" {
                    // Pay attention to the double spaces
                    let content = "<body><p>Test &lt;ok&gt;</p></body>";
                    let buf_reader = BufReader::new(content.as_bytes());
                    let mut generator = xml_extract_content(buf_reader, None);
                    let extracted_content = match generator.as_mut().resume() {
                        GeneratorState::Yielded(content) => content,
                        _ => panic!("Unexpected generator state"),
                    };
                    assert_eq!(extracted_content, "Test &lt;ok&gt;");
                }
            }

            describe "On a more complex and correct EPUB content" {
                it "it should extract the content correctly in 1 yield" {
                    // simple_2 contains a <h1>
                    // let file = std::fs::File::open("src/tests/simple_2.txt").unwrap();
                    let file = std::fs::File::open("src/tests/simple_1.txt").unwrap();
                    let file_reader = BufReader::new(file);
                    let mut lines_iter = file_reader.lines();
                    let content = lines_iter.next().unwrap().unwrap();
                    lines_iter.next();
                    let result = lines_iter.next().unwrap().unwrap();

                    println!("💁‍♀️ content simple_1: {}", content);
                    println!("🦀");
                    println!("💁‍♀️ result simple_1: {}", result);

                    let buf_reader = BufReader::new(content.as_bytes());

                    let mut generator = xml_extract_content(buf_reader, None);
                    let extracted_content = match generator.as_mut().resume() {
                        GeneratorState::Yielded(extracted_content) => extracted_content,
                        _ => panic!("Unexpected generator state"),
                    };

                    println!("💙");
                    println!("extracted content simple_1: {}", extracted_content);
                    assert_eq!(extracted_content, result);
                }
            }
        }

        describe "When the number of words in the EPUB content is > to nb_words_per_yield (several yields)" {
            describe "On a simple and correct EPUB content that is: nb_words_per_yield < length < 2 * nb_words_per_yield" {
                it "it should extract the content correctly in 2 yields, and complete" {
                    let content = "<html><head><title>Non-extracted title</title></head><body><p>Test</p>Ok - how are you?</body></html>";
                    let result_1 = "Test Ok - how";
                    let result_2 = "are you?";

                    let buf_reader = BufReader::new(content.as_bytes());
                    let mut generator = xml_extract_content(buf_reader, Some(4));

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
            }

            describe "On a more complex and correct EPUB content with: (x - 1) * nb_words_per_yield < number of words < x * nb_words_per_yield" {
                it "it should extract the content correctly in x yields" {
                    let expected_yielded_contents = vec!["It is nice to finally meet you.", "Would you like some coffee? I love", "coffee. I drink it every morning.", "!!!!!!!!", "Could you please pass me the sugar?"];
                    let expected_nb_yields = expected_yielded_contents.len();
                    let content = format!("<html><head><title>Test</title></head><body><p>{}</p></body></html>", expected_yielded_contents.join(" "));

                    let buf_reader = BufReader::new(content.as_bytes());
                    let mut generator = xml_extract_content(buf_reader, Some(8));

                    for i in 0..expected_nb_yields {
                        let yielded_extracted_content = match generator.as_mut().resume() {
                            GeneratorState::Yielded(content) => content,
                            _ => panic!("Unexpected generator state"),
                        };
                        assert_eq!(yielded_extracted_content, expected_yielded_contents[i]);
                    }
                }
            }
        }
    }
}
