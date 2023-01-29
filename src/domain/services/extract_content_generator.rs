use std::{io::{BufRead, BufReader, Read}, pin::Pin};

use genawaiter::{rc::gen, yield_, Generator, GeneratorState};

// TODO: HERE: use a generator to yield progressively the extracted content of the epub file

const HTML_ELEMENTS_NEEDING_A_SPACE: [&str; 11] = [
    "p",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "li",
    "ul",
    "ol",
    "blockquote",
];

const DEFAULT_NB_WORDS_PER_YIELD: usize = 100;

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
/// # Arguments
/// * `buf_reader` - A buffer to a content implementing Read. This buffer is moved to the function to avoid
///    reading in different places from the same buffer.
///
/// # Returns
/// A string containing the content of an epub file, without the HTML elements
///
/// # Examples
/// ```
/// use epub::domain::extract_content::extract_content;
///
/// let content = "<html><head><title>Test</title></head><body><p>Test</p></body></html>";
/// let extracted_content = extract_content(content);
/// assert_eq!(extracted_content, "Test");
/// ```
pub fn extract_content_generator<ContentToReadType>(
    buf_reader: BufReader<ContentToReadType>,
    nb_words_per_yield: Option<usize>,
) -> Pin<Box<dyn Generator<Yield = String, Return = Result<(), ()>>>>
where
    ContentToReadType: std::io::Read + 'static,
{
    let nb_words_per_yield = nb_words_per_yield.unwrap_or(DEFAULT_NB_WORDS_PER_YIELD);

    let generator = gen!({
        // let mut current_html_elements = std::collections::HashMap::<String, u32>::new();
        let mut current_extracted_content = String::new();

        let mut is_inside_body = false;
        let mut might_be_html_element = false;
        let mut previous_char_is_an_opening = false;

        // Stores a possible currently being extracted html element to:
        // - check if it is a `body` or any other html element
        // - or to add its content to the current_extracted_content if it's not an html element
        let mut current_possible_html_element = String::new();

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
                        previous_char_is_an_opening = true;
                        if is_inside_body {
                            current_extracted_content.push('<');
                            current_extracted_content.push_str(&current_possible_html_element);
                        }

                        current_possible_html_element = String::new();
                    } else if c == '>' {
                        if current_possible_html_element == "body" {
                            is_inside_body = true;
                        } else if current_possible_html_element == "/body" {
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

                        // TODO: handles the case where there is a < and a > but it's actual content

                        might_be_html_element = false;
                        previous_char_is_an_opening = false;
                        current_possible_html_element = String::new();
                    }
                    // A space is only possible at the end of an HTML element
                    else if previous_char_is_an_opening && c == ' ' {
                        might_be_html_element = false;
                        previous_char_is_an_opening = false;

                        // The previous string is part of the content of the epub
                        if is_inside_body {
                            current_extracted_content.push_str(&current_possible_html_element);
                        }
                        current_possible_html_element = String::new();
                    } else {
                        current_possible_html_element.push(c);
                        previous_char_is_an_opening = false;
                    }
                }
                // Needs to check if < represents the beginning of an HTML element, or if it's a simple character in the content
                else if c == '<' {
                    // The case with a second < is handled in the might_be_html_element block
                    previous_char_is_an_opening = true;
                    might_be_html_element = true;
                } else if is_inside_body {
                    // Avoids adding unecessary spaces
                    if c != ' '
                        || (current_extracted_content.len() > 0
                            && !current_extracted_content.ends_with(' '))
                    {
                        current_extracted_content.push(c);
                    }
                }
            }
        }

        // Removes last space if there is one
        if current_extracted_content.len() > 0 && current_extracted_content.ends_with(' ') {
            current_extracted_content.pop();
        }

        yield_!(current_extracted_content);
        Ok(())
    });

    Box::pin(generator)
}

#[cfg(test)]
mod tests {
    extern crate speculate;
    use speculate::speculate;

    use super::*;
    use std::io::{BufRead, BufReader, Read};

    speculate! {
        describe "extract_content_generator" {
            describe "On a simple and correct EPUB content" {
                it "it should extract the content correctly" {
                    let content = "<html><head><title>Test</title></head><body><p>Test</p></body></html>";
                    let buf_reader = BufReader::new(content.as_bytes());
                    let mut generator = extract_content_generator(buf_reader, None);
                    // TODO: HERE: last update: this is a bit complicated, no ?
                    let extracted_content = match generator.as_mut().resume() {
                        GeneratorState::Yielded(content) => content,
                        _ => panic!("Unexpected generator state"),
                    };
                    assert_eq!(extracted_content, "Test");
                }
            }


//             describe "On a multiline and correct EPUB content" {
//                 it "it should extract the content correctly" {
//                     let content = "\
// <html>
//     <head><title>Test</title></head>
//     <body>
//         <p>Test</p>
//     </body>
// </html>";
//                     let buf_reader = BufReader::new(content.as_bytes());
//                     let extracted_content = extract_content_generator(ExtractContentGeneratorArgs { buf_reader, nb_words_per_yield: None });
//                     assert_eq!(extracted_content, "Test");
//                 }
//             }

//             describe "On a more complex and correct EPUB content" {
//                 it "it should extract the content correctly" {
//                     let mut file = std::fs::File::open("src/tests/simple_1.txt").unwrap();
//                     let file_reader = BufReader::new(file);
//                     let mut lines_iter = file_reader.lines();
//                     let content = lines_iter.next().unwrap().unwrap();
//                     lines_iter.next();
//                     let result = lines_iter.next().unwrap().unwrap();

//                     println!("content simple_1: {}", content);
//                     println!("🦀");
//                     println!("result simple_1: {}", result);

//                     let buf_reader = BufReader::new(content.as_bytes());
//                     let extracted_content = extract_content_generator(ExtractContentGeneratorArgs { buf_reader, nb_words_per_yield: None });
//                     println!("💙");
//                     println!("extracted content simple_1: {}", extracted_content);
//                     assert_eq!(extracted_content, result);
//                 }
//             }
        }
    }
}
