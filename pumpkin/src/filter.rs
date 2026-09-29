//! Message filtering — the Bukkit v2 `TextFilter` ported to Pumpkin.
//!
//! Implements the upstream `chat.blockedWords` / `chat.filterReplacement`
//! behaviour from `settings.yml`:
//!
//! * matching is a **case-insensitive, non-overlapping scan** of the source
//!   text for every blocked word,
//! * every occurrence is replaced by `filterReplacement` repeated so that the
//!   replacement has the **same length as the matched word** (measured in
//!   UTF-16 code units, like Java `String.length()`).
//!
//! Additional length/rate guards live in the chat pipeline itself; this module
//! only maps words → censored text.

/// Filters chat text against the configured blocked words.
pub struct TextFilter {
    /// Lowercased blocked words.
    words: Vec<String>,
    /// Replacement unit (defaults to `"*"`).
    replacement: String,
}

impl TextFilter {
    /// Creates a filter. Empty words are ignored; `replacement` falls back to
    /// `"*"` when blank.
    pub fn new(words: &[String], replacement: &str) -> Self {
        let replacement = if replacement.is_empty() {
            "*".to_string()
        } else {
            replacement.to_string()
        };
        let words = words
            .iter()
            .filter(|w| !w.trim().is_empty())
            .map(|w| w.to_lowercase())
            .collect();
        Self { words, replacement }
    }

    /// True when the filter has at least one blocked word.
    pub fn is_active(&self) -> bool {
        !self.words.is_empty()
    }

    /// Censors `text`, returning the filtered copy.
    pub fn filter(&self, text: &str) -> String {
        if !self.is_active() {
            return text.to_string();
        }
        let lower = text.to_lowercase();
        let mut result = String::with_capacity(text.len());
        let mut index = 0usize; // byte offset into `lower` / `text`
        'outer: while index < text.len() {
            for word in &self.words {
                // Case-insensitive match anchored at `index` (words are
                // pre-lowercased; `lower` is the lowercased copy).
                if lower[index..].starts_with(word.as_str()) {
                    result.push_str(&self.replacement.repeat(word.encode_utf16().count()));
                    index += word.len();
                    continue 'outer;
                }
            }
            // No word matched here: copy one char (by UTF-8 boundary).
            let ch = text[index..].chars().next().expect("non-empty slice");
            result.push(ch);
            index += ch.len_utf8();
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn filter() -> TextFilter {
        TextFilter::new(&["bad".to_string(), "shit".to_string()], "*")
    }

    #[test]
    fn replaces_blocked_words_with_same_length() {
        assert_eq!(filter().filter("this is bad shit"), "this is *** ****");
    }

    #[test]
    fn matching_is_case_insensitive() {
        assert_eq!(filter().filter("BAD WORDS"), "*** WORDS");
    }

    #[test]
    fn non_overlapping_scan() {
        // `badbad` — one match at index 0, then the second scan starts after
        // it (~looks like the whole word). With a single anchored scan the
        // second `bad` is *not* consumed (it is part of the matched region).
        assert_eq!(filter().filter("badbad"), "******");
    }

    #[test]
    fn inactive_filter_passthrough() {
        let f = TextFilter::new(&[], "*");
        assert!(!f.is_active());
        assert_eq!(f.filter("anything"), "anything");
    }

    #[test]
    fn empty_replacement_defaults_to_star() {
        let f = TextFilter::new(&["x".to_string()], "");
        assert_eq!(f.filter("axb"), "a*b");
    }
}
