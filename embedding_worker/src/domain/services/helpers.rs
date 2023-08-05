use regex::Regex;
use once_cell::sync::Lazy;

/// Simple sentences splitter.
///
/// Simple regex:
/// - groups sentences finishing by .?! or if reaching the end of the content.
/// - removes sentences with less than 2 characters (ex: a `!` from several `!!!`)
pub fn split_sentences(content: &str) -> Vec<String> {
    // let re = Regex::new(r"\s*(?P<sentence>[^.!?]*(?:[.!?]|$))").unwrap();
    static RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"\s*(?P<sentence>[^.!?]*(?:[.!?]|$))").unwrap());
    RE.captures_iter(content)
        .map(|cap| cap["sentence"].to_string())
        .filter(|sentence| sentence.len() > 1)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn on_simple_sentences_it_splits_correctly() {
        let content = "Hello world. My name is Alex! Is this a test ?The end";
        let sentences = split_sentences(content);
        assert_eq!(
            sentences,
            vec![
                "Hello world.",
                "My name is Alex!",
                "Is this a test ?",
                "The end"
            ]
        )
    }

    #[test]
    fn on_sentences_with_successive_punctuation_marks_it_filters_them_out() {
        let content = "Hello world... Is this a test???The end...";
        let sentences = split_sentences(content);
        assert_eq!(
            sentences,
            vec!["Hello world.", "Is this a test?", "The end."]
        )
    }
}
