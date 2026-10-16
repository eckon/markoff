//! Names of task files and section directories: `010-fix-login-bug`.
//!
//! The numbered prefix sets the order, the slug is derived from the task title.
//! Everything here is pure string handling, no file system access.

/// Width of the order prefix. `ls` only sorts correctly while all prefixes have the same width.
const PREFIX_WIDTH: usize = 3;

/// Distance between neighbours: `010`, `020`, `030`, ... Smaller only for very full sections.
pub const ORDER_GAP: u32 = 10;

/// Highest order that fits into [`PREFIX_WIDTH`] digits.
pub const MAX_ORDER: u32 = 999;

/// Max slug length, so filenames stay readable in `ls`.
const MAX_SLUG_LEN: usize = 50;

/// Slug for titles without any usable characters, so a file is never named `010-.md`.
const FALLBACK_SLUG: &str = "untitled";

/// A name split into its parts: `010-fix-login-bug` -> order `10`, slug `fix-login-bug`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Name {
    pub order: u32,
    pub slug: String,
}

impl Name {
    /// Parses a file stem (without `.md`) or directory name.
    ///
    /// Returns `None` if there is no `NNN-` prefix. What to do with such files is up to the caller.
    pub fn parse(stem: &str) -> Option<Self> {
        let (prefix, slug) = stem.split_once('-')?;
        if prefix.len() != PREFIX_WIDTH || !prefix.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        Some(Self {
            order: prefix.parse().ok()?,
            slug: slug.to_string(),
        })
    }

    /// Formats back into `010-slug`, without extension.
    pub fn to_stem(&self) -> String {
        format!("{:0width$}-{}", self.order, self.slug, width = PREFIX_WIDTH)
    }
}

/// Turns a title into a filename-safe slug: `Äpfel kaufen!` -> `aepfel-kaufen`.
///
/// ASCII only, lowercase, `a-z0-9` separated by single `-`, at most [`MAX_SLUG_LEN`] chars.
/// Falls back to [`FALLBACK_SLUG`] if nothing is left.
pub fn slugify(title: &str) -> String {
    // German umlauts first, deunicode would turn `ä` into `a`.
    let title = title
        .replace('ä', "ae")
        .replace('ö', "oe")
        .replace('ü', "ue")
        .replace('Ä', "Ae")
        .replace('Ö', "Oe")
        .replace('Ü', "Ue")
        .replace('ß', "ss");
    let ascii = deunicode::deunicode(&title).to_lowercase();

    // Every run of non-alphanumeric chars becomes a single `-`.
    let mut slug = String::new();
    for word in ascii.split(|c: char| !c.is_ascii_alphanumeric()) {
        if word.is_empty() {
            continue;
        }
        if !slug.is_empty() {
            slug.push('-');
        }
        slug.push_str(word);
    }

    // Slug is pure ASCII, so cutting at any byte index is safe.
    slug.truncate(MAX_SLUG_LEN);
    let slug = slug.trim_end_matches('-');
    if slug.is_empty() {
        FALLBACK_SLUG.to_string()
    } else {
        slug.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_valid() {
        assert_eq!(
            Name::parse("010-fix-login-bug"),
            Some(Name {
                order: 10,
                slug: "fix-login-bug".to_string()
            })
        );
    }

    #[test]
    fn parse_invalid() {
        assert_eq!(Name::parse("fix-login-bug"), None);
        assert_eq!(Name::parse("10-fix"), None);
        assert_eq!(Name::parse("0100-fix"), None);
        assert_eq!(Name::parse("010"), None);
    }

    #[test]
    fn roundtrip() {
        let name = Name::parse("020-work").unwrap();
        assert_eq!(name.to_stem(), "020-work");
    }

    #[test]
    fn slugify_basic() {
        assert_eq!(slugify("Fix login bug"), "fix-login-bug");
        assert_eq!(slugify("  Call Bob!!  (urgent) "), "call-bob-urgent");
    }

    #[test]
    fn slugify_transliterates() {
        assert_eq!(slugify("Äpfel kaufen"), "aepfel-kaufen");
        assert_eq!(slugify("Straße"), "strasse");
        assert_eq!(slugify("Café crème"), "cafe-creme");
    }

    #[test]
    fn slugify_truncates_without_trailing_dash() {
        let slug = slugify(&"word ".repeat(20));
        assert!(slug.len() <= MAX_SLUG_LEN);
        assert!(!slug.ends_with('-'));
    }

    #[test]
    fn slugify_empty() {
        assert_eq!(slugify("!!!"), "untitled");
        assert_eq!(slugify(""), "untitled");
    }
}
