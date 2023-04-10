use genawaiter::{rc::gen, yield_, Generator};
use quick_xml::{events::Event, reader::Reader};
use std::{
    io::{BufRead, BufReader},
    pin::Pin,
};

const DEFAULT_NB_WORDS_PER_YIELD: usize = 100;

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
                            current_extracted_content.push(char::from(element.to_owned()));

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

#[cfg(test)]
mod tests {
    use genawaiter::GeneratorState;
    use super::*;

    /// When the input is empty
    #[test]
    fn when_the_input_is_empty_it_should_extract_an_empty_content_and_complete() {
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

    /// When the input is correctly formatted
    #[test]
    fn when_input_len_inf_nb_words_per_yield_it_should_yield_once() {
        let content = "<html><head><title>Test Title</title></head><body><p>Test in Body</p><p>Another text <a>inside link</a></p></body></html>";
        let buf_reader = BufReader::new(content.as_bytes());

        let mut generator = xml_extract_content(buf_reader, Some(100));

        let extracted_content = match generator.as_mut().resume() {
            GeneratorState::Yielded(content) => content,
            _ => panic!("Unexpected generator state"),
        };
        assert_eq!(extracted_content, "Test in BodyAnother text inside link");

        // Completes
        let extracted_result = match generator.as_mut().resume() {
            GeneratorState::Complete(result) => result,
            _ => panic!("Unexpected generator state"),
        };
        assert_eq!(extracted_result, Ok(()));
    }
}
