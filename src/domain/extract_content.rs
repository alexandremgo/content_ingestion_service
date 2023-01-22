/// Parse a string containing an epub content (with HTML element) into a string

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

/// Generates a string containing the content of an epub file, without the HTML element
///
/// Currently the epub content has to be utf-8 encoded.
/// Does not handle epub content containing HTML elements as the actual content of the epub.
///
/// TODO: remove any space between , . ; : ? ! and the previous and next words ?
///
/// # Arguments
/// * `content` - A string containing the content of an epub file, with HTML elements
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
pub fn extract_content(epub_content: &str) -> String {
    // let mut current_html_elements = std::collections::HashMap::<String, u32>::new();
    let mut extracted_content = String::new();

    let mut is_inside_body = false;
    let mut might_be_html_element = false;
    let mut previous_char_is_an_opening = false;

    let mut current_possible_html_element = String::new();

    // Reads characters one by one
    // TODO: careful/check when none utf-8 characters
    // Only keep the characters that are not inside an HTML element
    // But inside the body element
    for c in epub_content.chars() {
        if might_be_html_element {
            // An opening < has already been seen. If among the next characters there is a new opening <,
            // then the string since the previous < is part of the content of the epub. And the new < could be an html element.
            if c == '<' {
                previous_char_is_an_opening = true;
                if is_inside_body {
                    extracted_content.push('<');
                    extracted_content.push_str(&current_possible_html_element);
                }

                current_possible_html_element = String::new();
            } else if c == '>' {
                if current_possible_html_element == "body" {
                    is_inside_body = true;
                } else if current_possible_html_element == "/body" {
                    is_inside_body = false;
                }

                // Adds a space if the HTML element is am opening paragraph, a header etc.
                if is_inside_body
                    && HTML_ELEMENTS_NEEDING_A_SPACE
                        .contains(&current_possible_html_element.as_str())
                    && extracted_content.len() > 0
                    && !extracted_content.ends_with(' ')
                {
                    extracted_content.push(' ');
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
                    extracted_content.push_str(&current_possible_html_element);
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
            extracted_content.push(c);
        }
    }

    extracted_content
}

#[cfg(test)]
mod tests {
    extern crate speculate;
    use speculate::speculate;

    use super::*;
    use std::io::{BufRead, BufReader, Read};

    speculate! {
        describe "extract_content" {
            describe "On a simple and correct EPUB content" {
                it "it should extract the content correctly" {
                    let content = "<html><head><title>Test</title></head><body><p>Test</p></body></html>";
                    let extracted_content = extract_content(content);
                    assert_eq!(extracted_content, "Test");
                }
            }

            describe "On a more complex and correct EPUB content" {
                it "it should extract the content correctly" {
                    let mut file = std::fs::File::open("src/tests/simple_1.txt").unwrap();
                    let file_reader = BufReader::new(file);
                    let mut lines_iter = file_reader.lines();
                    let content = lines_iter.next().unwrap().unwrap();
                    lines_iter.next();
                    let result = lines_iter.next().unwrap().unwrap();

                    println!("content simple_1: {}", content);
                    println!("🦀");
                    println!("result simple_1: {}", result);

                    let extracted_content = extract_content(&content);
                    println!("💙");
                    println!("extracted content simple_1: {}", extracted_content);
                    assert_eq!(extracted_content, result);
                }
            }
        }
    }
}
