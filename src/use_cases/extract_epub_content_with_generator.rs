use std::pin::Pin;

use genawaiter::{rc::gen, yield_, Generator};

use crate::ports::source_buffer_port::SourceBufferPort;

const HTML_ELEMENTS_NEEDING_A_SPACE: [&str; 11] = [
    "<p",
    "<h1",
    "<h2",
    "<h3",
    "<h4",
    "<h5",
    "<h6",
    "<li",
    "<ul",
    "<ol",
    "<blockquote",
];

const SPECIAL_CHARS_FOR_COUNTING_WORDS: [char; 6] = [',', '.', ';', ':', '?', '!'];

const DEFAULT_NB_WORDS_PER_YIELD: usize = 100;
const LIMIT_NB_WORDS_IN_HTML_ELEMENT: usize = 30;

pub struct Request {
    // source_file_path: String,
    nb_words_per_yield: Option<usize>,
}

pub struct Dependencies {
    source_buffer: Box<dyn SourceBufferPort>,
}

pub fn execute(
    dependencies: Dependencies,
    request: Request,
) -> Pin<Box<dyn Generator<Yield = String, Return = Result<(), ()>>>> {
    let Request { nb_words_per_yield } = request;
    let Dependencies { mut source_buffer } = dependencies;

    let nb_words_per_yield = nb_words_per_yield.unwrap_or(DEFAULT_NB_WORDS_PER_YIELD);

    let generator = gen!({
        // let mut current_html_elements = std::collections::HashMap::<String, u32>::new();
        let mut current_extracted_content = String::new();

        let mut is_inside_body = false;
        let mut might_be_html_element = false;

        // Stores a possible currently being extracted html element to:
        // - check if it is a `body` or any other html element
        // - or to add its content to the current_extracted_content if it's not an html element
        let mut current_possible_html_element = String::new();
        let mut current_possible_html_element_nb_spaces = 0;

        let mut current_nb_words = 0;

        // Reads lines one by one, and for each line, read characters one by one
        // TODO: careful/check when none utf-8 characters
        // Extracted content: only keeps the characters that are not HTML elements
        // and that are contained inside the <body> elements
        // Performance: O(n) where n is the number of characters in the epub content
        loop {
            let source_char = match source_buffer.next() {
                Ok(result) => match result {
                    None => break,
                    Some(source_char) => source_char,
                },
                Err(error) => {
                    panic!("An error occurred: {:?}", error);
                }
            };
            let c = source_char.value;

            if might_be_html_element {
                // An opening < has already been seen. If among the next characters there is a new opening <,
                // then the string since the previous < is part of the content of the epub.
                // And the new < could be an html element.
                if c == '<' {
                    if is_inside_body {
                        current_extracted_content.push_str(&current_possible_html_element);
                        // 1 for < and 1 for each space after each word
                        current_nb_words += current_possible_html_element_nb_spaces + 1;
                    }

                    current_possible_html_element_nb_spaces = 0;
                    current_possible_html_element = String::new();
                    current_possible_html_element.push('<');
                } else if c == '>' {
                    if current_possible_html_element == "<body" {
                        is_inside_body = true;
                    } else if current_possible_html_element == "</body" {
                        is_inside_body = false;
                    }

                    // Adds a space if the HTML element is an opening paragraph, a header etc.
                    if is_inside_body
                        && HTML_ELEMENTS_NEEDING_A_SPACE
                            .contains(&current_possible_html_element.as_str())
                        && current_extracted_content.len() > 0
                        && !current_extracted_content.ends_with(' ')
                    {
                        current_extracted_content.push(' ');
                    }

                    // TODO: better handle the case where there is a < and a > but it's actual content

                    might_be_html_element = false;
                    current_possible_html_element = String::new();
                }
                // Protects against an opening < followed by too many words
                // Possible to improve this a lot.
                else if current_possible_html_element_nb_spaces > LIMIT_NB_WORDS_IN_HTML_ELEMENT {
                    current_possible_html_element.push(c);

                    if is_inside_body {
                        current_extracted_content.push_str(&current_possible_html_element);
                        // 1 for < and 1 for each space after each word
                        current_nb_words += current_possible_html_element_nb_spaces + 1;
                    }

                    current_possible_html_element_nb_spaces = 0;
                    might_be_html_element = false;
                    current_possible_html_element = String::new();
                } else if c == ' ' {
                    // A space is not possible just after the HTML openening, or after another one
                    // TODO: currently does not handle element with props correctly
                    // TODO: And if a "<word" happens, current_extracted_content could become huge until it is stopped
                    if current_possible_html_element.ends_with('<')
                        || current_possible_html_element.ends_with(' ')
                    {
                        might_be_html_element = false;

                        // The previous string is part of the content of the epub
                        if is_inside_body {
                            // Adds the space if the previous character was not a space
                            if !current_possible_html_element.ends_with(' ') {
                                current_possible_html_element.push(' ');
                            }
                            current_extracted_content.push_str(&current_possible_html_element);
                            // 1 for < and 1 for each space after each word
                            current_nb_words += current_possible_html_element_nb_spaces + 1;
                        }

                        current_possible_html_element = String::new();
                    } else {
                        current_possible_html_element_nb_spaces += 1;
                        current_possible_html_element.push(c);
                    }
                } else {
                    current_possible_html_element.push(c);
                }
            }
            // Needs to check if < represents the beginning of an HTML element, or if it's a simple character in the content
            else if c == '<' {
                // The case with a second < is handled in the might_be_html_element block
                current_possible_html_element.push('<');
                might_be_html_element = true;
            } else if is_inside_body {
                // Avoids adding unecessary spaces
                if c == ' '
                    && current_extracted_content.len() > 0
                    && !current_extracted_content.ends_with(' ')
                {
                    // A special char is already counted when iterating on it. A space after a special char does not represents a new word
                    if !current_extracted_content.ends_with(&SPECIAL_CHARS_FOR_COUNTING_WORDS) {
                        current_nb_words += 1;
                    }

                    current_extracted_content.push(c);
                } else if SPECIAL_CHARS_FOR_COUNTING_WORDS.contains(&c) {
                    if current_extracted_content.len() == 0
                        || current_extracted_content.ends_with(' ')
                        || current_extracted_content.ends_with(&SPECIAL_CHARS_FOR_COUNTING_WORDS)
                    {
                        current_nb_words += 1;
                    } else {
                        current_nb_words += 2;
                    }

                    current_extracted_content.push(c);
                } else if c != ' ' {
                    current_extracted_content.push(c);
                }
            }

            // Yields the current_extracted_content if it contains enough words
            // TODO: handle the case where not an html element but the content was bigger than the max number of words
            // Cannot just loop and slice because it needs to count the number of words and special chars
            if current_nb_words >= nb_words_per_yield {
                current_extracted_content = clean_content_before_yield(&current_extracted_content);

                yield_!(current_extracted_content);
                current_extracted_content = String::new();
                current_nb_words = 0;
            }
        }

        current_extracted_content = clean_content_before_yield(&current_extracted_content);

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

fn clean_content_before_yield(content: &str) -> String {
    let cleaned_content;

    // Removes last space if there is one
    if content.len() > 0 && content.ends_with(' ') {
        cleaned_content = content[..content.len() - 1].to_string();
    } else {
        cleaned_content = content.to_string();
    }

    cleaned_content
}

#[cfg(test)]
mod tests {
    extern crate speculate;
    use speculate::speculate;

    use super::*;
    use crate::adapters::epub_source_buffer::EpubSourceBuffer;
    use genawaiter::GeneratorState;

    speculate! {
        describe "extract_epub_content_with_generator" {
            it "should work" {
                let source_buffer = EpubSourceBuffer::try_new(String::from("src/tests/accessible_epub_3.epub")).unwrap();
                // let source_buffer = EpubSourceBuffer::try_new(String::from("src/tests/minimal_sample.epub")).unwrap();

                println!("🔮🔮🔮🔮🔮🔮🔮 Let's go");


                let mut generator = execute(Dependencies { source_buffer: Box::new(source_buffer) }, Request { nb_words_per_yield: Some(100) });

                for i in 0..30 {
                    let yielded_extracted_content = match generator.as_mut().resume() {
                        GeneratorState::Yielded(content) => content,
                        _ => panic!("Unexpected generator state"),
                    };
                    // assert_eq!(yielded_extracted_content, expected_yielded_contents[i]);
                    println!("🦖 yield:\n\n{}", yielded_extracted_content);
                }

                // TODO: will need another watchdog so we don't run infinitely if there is a loop
                // for _i in 0..1000000 {
                // loop {
                //     let c = match source_buffer.next() {
                //         Ok(result) => match result {
                //             None => break,
                //             Some(source_char) => source_char
                //         },
                //         Err(error) => {
                //             panic!("An error occurred: {:?}", error);
                //         }
                //     };
                //     print!("{}", c.value);
                // }

                println!("🔮🔮🔮🔮🔮🔮🔮 THE END");
                assert_eq!(1, 2);
            }
        }
    }
}
