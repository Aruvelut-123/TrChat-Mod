//! Message filtering — the Bukkit v2 `MessageGuard` + `TextFilter` ported to
//! Pumpkin.
//!
//! Two independent layers, mirroring the Mod's chat pipeline:
//!
//! * [`MessageGuard`] — the `settings.yml` `chat.blockedWords` /
//!   `chat.filterReplacement` quick path: a case-insensitive, non-overlapping
//!   scan where every occurrence is replaced by `filterReplacement` repeated
//!   to the **same length as the matched word** (measured in UTF-16 code
//!   units, like Java `String.length()`).
//! * [`TextFilter`] — the `filter.yml` profile (`FilterService` / `TextFilter`
//!   in the Mod): a local word list matched while **skipping ignored
//!   punctuation**, full-width → half-width **normalization**, a **white list**
//!   of phrases whose occurrences are protected from replacement, and a single
//!   `Replacement` char applied per output position.

use std::collections::HashSet;

/// Filters chat text against the configured blocked words (`settings.yml`).
pub struct MessageGuard {
    /// Lowercased blocked words.
    words: Vec<String>,
    /// Replacement unit (defaults to `"*"`).
    replacement: String,
}

impl MessageGuard {
    /// Creates a guard. Empty words are ignored; `replacement` falls back to
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

    /// True when the guard has at least one blocked word.
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
        let mut index = 0;
        'outer: while index < text.len() {
            // Case-insensitive match anchored at `index` (words are
            // pre-lowercased; `lower` is the lowercased copy).
            for word in &self.words {
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

/// Filters chat text against the `filter.yml` profile.
pub struct TextFilter {
    /// Lowercased, full-width-normalized words, longest first (the Mod sorts
    /// by length descending so longer words win).
    words: Vec<Vec<char>>,
    /// Ignored punctuation, lowercased raw (the Mod stores them without the
    /// full-width → half-width normalization and normalizes on lookup).
    punctuation: HashSet<char>,
    /// White-list phrases, lowercased; their occurrences are protected.
    white_list: Vec<Vec<char>>,
    /// Replacement char.
    replacement: char,
}

impl TextFilter {
    /// Creates a filter from the `filter.yml` profile. Blank words are
    /// dropped; punctuation/white-list entries are lowercased like the Mod.
    pub fn new(
        words: &[String],
        punctuation: &[char],
        white_list: &[String],
        replacement: char,
    ) -> Self {
        let words = words
            .iter()
            .filter(|w| !w.trim().is_empty())
            .map(|w| {
                w.to_lowercase()
                    .chars()
                    .map(normalize)
                    .collect::<Vec<char>>()
            })
            .collect::<HashSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        let mut words = words;
        words.sort_by(|a, b| b.len().cmp(&a.len()));
        let punctuation = punctuation
            .iter()
            .map(|c| c.to_lowercase().next().unwrap_or(*c))
            .collect();
        let white_list = white_list
            .iter()
            .filter(|p| !p.is_empty())
            .map(|p| p.to_lowercase().chars().collect::<Vec<char>>())
            .collect();
        Self {
            words,
            punctuation,
            white_list,
            replacement,
        }
    }

    /// True when the filter has at least one blocked word.
    pub fn is_active(&self) -> bool {
        !self.words.is_empty()
    }

    /// Censors `text` (`filter(input).text()` in the Mod).
    pub fn filter(&self, text: &str) -> String {
        self.filter_with_count(text).0
    }

    /// Censors `text` and returns the number of matched words
    /// (`filter(input).matches()` in the Mod — used by the anvil guard).
    pub fn filter_with_count(&self, text: &str) -> (String, usize) {
        let chars: Vec<char> = text.chars().collect();
        if chars.is_empty() || self.words.is_empty() {
            return (text.to_string(), 0);
        }
        let lower: Vec<char> = text.to_lowercase().chars().collect();
        let protected = protected_characters(&lower, &self.white_list);
        let mut output = chars.clone();
        let mut matches = 0;
        let mut start = 0;
        while start < output.len() {
            if protected[start] || self.punctuation.contains(&normalize(output[start])) {
                start += 1;
                continue;
            }
            if let Some(end) =
                try_match_word(&lower, start, &self.words, &self.punctuation, &protected)
            {
                for index in start..=end {
                    if !self.punctuation.contains(&normalize(output[index])) {
                        output[index] = self.replacement;
                    }
                }
                matches += 1;
                start = end + 1;
            } else {
                start += 1;
            }
        }
        (output.into_iter().collect(), matches)
    }
}

/// Full-width → half-width normalization plus lowercase, mirroring the Mod's
/// `TextFilter.normalize(char)` (U+3000 → space, U+FF01–U+FF5E → ASCII).
fn normalize(c: char) -> char {
    let cp = c as u32;
    if cp == 0x3000 {
        ' '
    } else if (0xFF01..=0xFF5E).contains(&cp) {
        char::from_u32(cp - 0xFEE0).unwrap_or(c)
    } else {
        c.to_lowercase().next().unwrap_or(c)
    }
}

/// Marks every occurrence of every white-list phrase as protected (the Mod's
/// `protectedCharacters`), using a plain case-insensitive region search on
/// the lowercased text (no punctuation skipping / normalization).
fn protected_characters(lower: &[char], white_list: &[Vec<char>]) -> Vec<bool> {
    let mut protected = vec![false; lower.len()];
    for needle in white_list {
        if needle.is_empty() {
            continue;
        }
        let mut from = 0;
        while from + needle.len() <= lower.len() {
            if lower[from..from + needle.len()] == needle[..] {
                for slot in protected.iter_mut().take(from + needle.len()).skip(from) {
                    *slot = true;
                }
                from += needle.len();
            } else {
                from += 1;
            }
        }
    }
    protected
}

/// Tries every word at `start` (skipping punctuation, stopping at protected
/// positions); returns the end index of the first word that fully matches.
fn try_match_word(
    lower: &[char],
    start: usize,
    words: &[Vec<char>],
    punctuation: &HashSet<char>,
    protected: &[bool],
) -> Option<usize> {
    'words: for word in words {
        let mut input_index = start;
        let mut word_index = 0;
        while input_index < lower.len() && word_index < word.len() {
            if protected[input_index] {
                // The Mod's `match` returns null → try the next word.
                continue 'words;
            }
            let actual = normalize(lower[input_index]);
            if punctuation.contains(&actual) {
                input_index += 1;
                continue;
            }
            if actual != word[word_index] {
                continue 'words;
            }
            input_index += 1;
            word_index += 1;
        }
        if word_index == word.len() {
            return Some(input_index - 1);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn guard() -> MessageGuard {
        MessageGuard::new(&["bad".to_string(), "shit".to_string()], "*")
    }

    #[test]
    fn replaces_blocked_words_with_same_length() {
        assert_eq!(guard().filter("this is bad shit"), "this is *** ****");
    }

    #[test]
    fn matching_is_case_insensitive() {
        assert_eq!(guard().filter("BAD WORDS"), "*** WORDS");
    }

    #[test]
    fn non_overlapping_scan() {
        assert_eq!(guard().filter("badbad"), "******");
    }

    #[test]
    fn inactive_guard_passthrough() {
        let f = MessageGuard::new(&[], "*");
        assert!(!f.is_active());
        assert_eq!(f.filter("anything"), "anything");
    }

    #[test]
    fn empty_replacement_defaults_to_star() {
        let f = MessageGuard::new(&["x".to_string()], "");
        assert_eq!(f.filter("axb"), "a*b");
    }

    fn tf() -> TextFilter {
        TextFilter::new(
            &["nmsl".to_string(), "fuck".to_string()],
            &['!', '.', '，', '。'],
            &["has been".to_string()],
            '*',
        )
    }

    #[test]
    fn skips_punctuation_between_word_chars() {
        // `f.u.c.k` with dots — punctuation is skipped while matching, and the
        // punctuation itself is left untouched by the replacement pass.
        assert_eq!(tf().filter("f.u.c.k"), "*.*.*.*");
    }

    #[test]
    fn full_width_normalized_match() {
        // Full-width `ｆｕｃｋ` normalizes to `fuck` and matches.
        assert_eq!(tf().filter("ｆｕｃｋ"), "****");
    }

    #[test]
    fn whitelist_protects_occurrences() {
        // `has been` is white-listed, so the `has been` region stays intact
        // even though it sits inside a blocked word match region.
        assert_eq!(tf().filter("this has been fucked"), "this has been ****ed");
    }

    #[test]
    fn longer_word_wins() {
        let f = TextFilter::new(&["fuck".to_string(), "fucker".to_string()], &[], &[], '*');
        assert_eq!(f.filter("fucker"), "******");
    }

    #[test]
    fn inactive_filter_passthrough() {
        let f = TextFilter::new(&[], &[], &[], '*');
        assert!(!f.is_active());
        assert_eq!(f.filter("anything"), "anything");
        assert_eq!(f.filter_with_count("anything"), ("anything".to_string(), 0));
    }
}
