use std::{
    io::{BufRead, BufReader, Read},
    pin::Pin,
};

use genawaiter::{rc::gen, yield_, Generator};

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

// TODO: handle html elements with properties
// Could also add: current_possible_html_element etc.
// struct HtmlElementState {
//     pub number_spaces: u32,
//     pub is_property_name: bool,
//     // Transitions to true on '=' and is_property_name === true.
//     // Transitions to false on ' '
//     pub is_property_value: bool,
// }

/// Excrats the content (without the HTML elements) of an epub fileontent of an epub file, by returning
/// a generator that yields the content progressively.
///
/// The length (in words) of each yeild can be configured.
///
/// Currently the epub content has to be utf-8 encoded.
/// Does not handle epub content containing HTML elements as the actual content of the epub.
///
/// TODO: remove any space between , . ; : ? ! and the previous and next words ?
///
/// To be able to move the Generator, genawaiter::rc is used.
///
/// # Arguments
/// * `buf_reader` - A buffer to a content implementing Read. This buffer is moved to the function to avoid
///    reading in different places from the same buffer.
///
/// # Returns
/// A generator that yields the content of the epub progressively.
/// The generator is wrapped in a Pin<Box<...>> because, like Future, a Generator can hold a reference into another field of
/// the same struct (becoming a self-referential type). If the Generator is moved, then the reference is incorrect.
/// Pinning the generator to a particular spot in memory prevents this problem, making it safe to create references
/// to values inside the generator block.
///
/// # Examples
/// ```
/// use epub::domain::extract_content::extract_content;
///
/// let content = "<html><head><title>Test</title></head><body><p>Test</p></body></html>";
/// let extracted_content = extract_content(content);
/// assert_eq!(extracted_content, "Test");
/// ```
//
pub fn extract_content_generator<'box_lt, ContentToReadType>(
    buf_reader: BufReader<ContentToReadType>,
    nb_words_per_yield: Option<usize>,
) -> Pin<Box<dyn Generator<Yield = String, Return = Result<(), ()>> + 'box_lt>>
where
    ContentToReadType: std::io::Read + 'box_lt,
{
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
        for line in buf_reader.lines() {
            let line = line.unwrap();
            println!("🥐 line: {}", line);

            for c in line.chars() {
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
                    else if current_possible_html_element_nb_spaces
                        > LIMIT_NB_WORDS_IN_HTML_ELEMENT
                    {
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
                            || current_extracted_content
                                .ends_with(&SPECIAL_CHARS_FOR_COUNTING_WORDS)
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
                    current_extracted_content =
                        clean_content_before_yield(&current_extracted_content);

                    yield_!(current_extracted_content);
                    current_extracted_content = String::new();
                    current_nb_words = 0;
                }
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
    use genawaiter::GeneratorState;
    use std::io::{BufRead, BufReader, Read};

    speculate! {
        describe "extract_content_generator" {
            describe "On an empty EPUB content" {
                it "it should extract an empty content" {
                    let content = "";
                    let buf_reader = BufReader::new(content.as_bytes());
                    let mut generator = extract_content_generator(buf_reader, None);
                    let extracted_content = match generator.as_mut().resume() {
                        GeneratorState::Yielded(content) => content,
                        _ => panic!("Unexpected generator state"),
                    };
                    assert_eq!(extracted_content, "");
                }
            }

            describe "When the number of words in the EPUB content is < to nb_words_per_yield (only 1 yield)" {
                describe "On a simple and correct EPUB content" {
                    it "it should extract the content correctly in 1 yield" {
                        let content = "<html><head><title>Test</title></head><body><p>Test</p></body></html>";
                        let buf_reader = BufReader::new(content.as_bytes());
                        let mut generator = extract_content_generator(buf_reader, None);
                        let extracted_content = match generator.as_mut().resume() {
                            GeneratorState::Yielded(content) => content,
                            _ => panic!("Unexpected generator state"),
                        };
                        assert_eq!(extracted_content, "Test");
                    }
                }


                describe "On a multiline and correct EPUB content" {
                    it "it should extract the content correctly in 1 yield" {
                        let content = "\
    <html>
        <head><title>Test</title></head>
        <body>
            <p>Test</p>
        </body>
    </html>";
                        let buf_reader = BufReader::new(content.as_bytes());

                        let mut generator = extract_content_generator(buf_reader, None);
                        let extracted_content = match generator.as_mut().resume() {
                            GeneratorState::Yielded(content) => content,
                            _ => panic!("Unexpected generator state"),
                        };
                        assert_eq!(extracted_content, "Test");
                    }
                }

                describe "On an EPUB content with some opening < chars not representing an HTML element" {
                    it "it should extract the content correctly in 1 yield" {
                        // Pay attention to the double spaces
                        let content = "< incorrect<incorrect  <body><p>Test <partOfContent  < partOfContentToo <ok</p></body>";
                        let buf_reader = BufReader::new(content.as_bytes());
                        let mut generator = extract_content_generator(buf_reader, None);
                        let extracted_content = match generator.as_mut().resume() {
                            GeneratorState::Yielded(content) => content,
                            _ => panic!("Unexpected generator state"),
                        };
                        assert_eq!(extracted_content, "Test <partOfContent < partOfContentToo <ok");
                    }

                    describe "and the opening < char is followed by a long string" {
                        it "it should extract the content correctly in 1 yield" {
                            let too_many_words = "a ".repeat(37).trim_end().to_string();
                            let expected_content = format!("It is <{too_many_words}");
                            let content = format!("<body>{expected_content}</body>");

                            let buf_reader = BufReader::new(content.as_bytes());
                            let mut generator = extract_content_generator(buf_reader, None);
                            let extracted_content = match generator.as_mut().resume() {
                                GeneratorState::Yielded(content) => content,
                                _ => panic!("Unexpected generator state"),
                            };
                            assert_eq!(extracted_content, expected_content);
                        }
                    }
                }


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

                        let mut generator = extract_content_generator(buf_reader, None);
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

            describe "When the number of words in the EPUB content is > to nb_words_per_yield (more than 1 yield)" {
                describe "On a simple and correct EPUB content with: nb_words_per_yield < number of words < 2 * nb_words_per_yield" {
                    it "it should extract the content correctly in 2 yields" {
                        let content = "<html><head><title>Test</title></head><body><p>It is nice to finally meet you. Would you like some coffee ?</p></body></html>";
                        let buf_reader = BufReader::new(content.as_bytes());
                        let mut generator = extract_content_generator(buf_reader, Some(8));

                        let yielded_extracted_content = match generator.as_mut().resume() {
                            GeneratorState::Yielded(content) => content,
                            _ => panic!("Unexpected generator state"),
                        };
                        assert_eq!(yielded_extracted_content, "It is nice to finally meet you.");

                        let yielded_extracted_content = match generator.as_mut().resume() {
                            GeneratorState::Yielded(content) => content,
                            _ => panic!("Unexpected generator state"),
                        };
                        assert_eq!(yielded_extracted_content, "Would you like some coffee ?");
                    }
                }

                describe "On a more complex and correct EPUB content with: (x - 1) * nb_words_per_yield < number of words < x * nb_words_per_yield" {
                    it "it should extract the content correctly in x yields" {
                        let expected_yielded_contents = vec!["It is nice to finally meet you.", "Would you like some coffee? I love", "coffee. I drink it every morning.", "!!!!!!!!", "Could you please pass me the sugar?"];
                        let expected_nb_yields = expected_yielded_contents.len();
                        let content = format!("<html><head><title>Test</title></head><body><p>{}</p></body></html>", expected_yielded_contents.join(" "));

                        let buf_reader = BufReader::new(content.as_bytes());
                        let mut generator = extract_content_generator(buf_reader, Some(8));

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
}
