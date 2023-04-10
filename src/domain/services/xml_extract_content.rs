use genawaiter::{rc::gen, yield_, Generator};
use quick_xml::{events::Event, reader::Reader};
use std::{
    io::{BufRead, BufReader},
    pin::Pin,
};

const DEFAULT_NB_WORDS_PER_YIELD: usize = 100;

const UNWANTED_CHARS: [char; 1] = ['\n'];

/// BufReader provides buffering capabilities: tt reads data from an underlying reader in larger chunks to reduce
/// the number of read calls made to the underlying reader, which can improve performance.
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
    let mut previous_char_was_space = false;

    let generator = gen!({
        // The `Reader` does not implement `Iterator` because it outputs borrowed data (`Cow`s)
        loop {
            // NOTE: this is the generic case when we don't know about the input BufRead.
            // when the input is a &str or a &[u8], we don't actually need to use another
            // buffer, we could directly call `reader.read_event()`
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
                    _ => (),
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

                            // Trims unnecessary spaces
                            if current_char == ' ' {
                                if previous_char_was_space || current_extracted_content.len() == 0 {
                                    continue;
                                } else {
                                    previous_char_was_space = true;
                                }
                            } else {
                                previous_char_was_space = false;
                            }

                            current_extracted_content.push(current_char);

                            // TODO: count number of words, not number of chars ...
                            if current_extracted_content.len() >= nb_words_per_yield {
                                // let current_extracted_content = String::from_utf8(current_u8_document.to_owned()).unwrap();
                                println!("🧠 current document: {:?}", current_extracted_content);

                                yield_!(current_extracted_content);

                                // The value is moved in yield_! above, and could not also be moved in the final yield_!
                                // So for now it needs to be cleared with a new String.
                                current_extracted_content = String::new();
                                // Keeps the memory capacity of the vector/String
                                // current_extracted_content.clear();
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

        println!("🧠 current document: {:?}", current_extracted_content);
        // Yields the remaining content
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

        // TODO: count number of words, not number of chars ...
        describe "On a more complex and correct EPUB content" {
            it "it should extract the content correctly in 1 yield" {
                let file = std::fs::File::open("src/tests/simple_1.txt").unwrap();
                let file_reader = BufReader::new(file);
                let mut lines_iter = file_reader.lines();
                let content = lines_iter.next().unwrap().unwrap();
                lines_iter.next();
                let result = lines_iter.next().unwrap().unwrap();

                println!("content simple_1: {}", content);
                println!("🦀");
                println!("result simple_1: {}", result);

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
  }
}
