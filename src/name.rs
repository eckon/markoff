//! Names of task files and section directories: `fix-login-bug`, derived from the title.
//!
//! Everything here is pure string handling, no file system access.

/// Max slug length, so filenames stay readable in `ls`.
const MAX_SLUG_LEN: usize = 50;

/// Slug for titles without any usable characters, so a file is never named `.md`.
const FALLBACK_SLUG: &str = "untitled";

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
