//! The `/` filter: case-insensitive substring matching. Pure string handling.

use std::ops::Range;

/// Whether `query` occurs in `text`, ignoring case. An empty query matches everything.
pub fn matches(text: &str, query: &str) -> bool {
    query.is_empty()
        || text
            .char_indices()
            .any(|(start, _)| match_len(&text[start..], query).is_some())
}

/// Byte ranges of all non-overlapping matches of `query` in `text`, ignoring case. For the
/// highlight in the preview. Empty for an empty query.
pub fn find(text: &str, query: &str) -> Vec<Range<usize>> {
    if query.is_empty() {
        return Vec::new();
    }
    let mut ranges: Vec<Range<usize>> = Vec::new();
    for (start, _) in text.char_indices() {
        if ranges.last().is_some_and(|last| start < last.end) {
            continue;
        }
        if let Some(len) = match_len(&text[start..], query) {
            ranges.push(start..start + len);
        }
    }
    ranges
}

/// Byte length of the start of `text` that equals `query` ignoring case, `None` if it does not.
///
/// Compared char by char, not via `to_lowercase` of the whole text: lowercasing can change the
/// byte length, then the ranges would no longer fit the original text.
fn match_len(text: &str, query: &str) -> Option<usize> {
    let mut chars = text.char_indices();
    for q in query.chars() {
        let (_, c) = chars.next()?;
        if !c.to_lowercase().eq(q.to_lowercase()) {
            return None;
        }
    }
    Some(chars.next().map_or(text.len(), |(end, _)| end))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_ignores_case() {
        assert!(matches("Fix login bug", "LOGIN"));
        assert!(matches("Äpfel kaufen", "äpfel"));
        assert!(!matches("Fix login bug", "logout"));
        assert!(matches("anything", ""));
        assert!(!matches("", "x"));
    }

    #[test]
    fn find_returns_byte_ranges() {
        assert_eq!(find("Fix login, LOGIN again", "login"), [4..9, 11..16]);
        // `ä` takes two bytes, the range still fits the original text.
        assert_eq!(find("Bär bär", "BÄR"), [0..4, 5..9]);
        assert_eq!(find("aaaa", "aa"), [0..2, 2..4], "no overlaps");
        assert_eq!(find("text", ""), []);
    }
}
