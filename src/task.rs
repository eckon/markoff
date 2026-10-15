//! In-memory model of the todo tree: sections that contain tasks.
//!
//! Built from paths and file contents, no file system access here.

use std::path::{Path, PathBuf};

use crate::name::Name;

/// One task file, e.g. `todo/010-work/010-fix-login-bug.md`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Task {
    pub path: PathBuf,
    /// `None` if the filename has no `NNN-` prefix (created by hand).
    pub name: Option<Name>,
    /// Text of the `# Heading`, `None` if the file has none.
    pub heading: Option<String>,
    /// Everything after the heading. The whole file if there is no heading.
    pub body: String,
}

impl Task {
    pub fn parse(path: PathBuf, content: &str) -> Self {
        let name = stem(&path).and_then(Name::parse);
        let (heading, body) = split_heading(content);
        Self {
            path,
            name,
            heading,
            body: body.to_string(),
        }
    }

    /// The heading, or the filename without prefix if there is none: `010-call-bob.md` -> `call bob`.
    pub fn title(&self) -> String {
        if let Some(heading) = &self.heading {
            return heading.clone();
        }
        let fallback = match &self.name {
            Some(name) => name.slug.as_str(),
            None => stem(&self.path).unwrap_or_default(),
        };
        fallback.replace('-', " ")
    }
}

/// A directory of tasks. The inbox (`todo/` itself) has an empty path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section {
    /// Section names without prefix: `["work"]`. A `Vec` so nesting can be added later.
    pub path: Vec<String>,
    pub dir: PathBuf,
    pub tasks: Vec<Task>,
}

impl Section {
    /// Display name, also used as the archive heading: `work`, later `private/garden`.
    pub fn title(&self) -> String {
        if self.path.is_empty() {
            "inbox".to_string()
        } else {
            self.path.join("/")
        }
    }
}

fn stem(path: &Path) -> Option<&str> {
    path.file_stem()?.to_str()
}

/// Splits `# Title\n\nbody` into `(Some("Title"), "body")`.
///
/// Only the first non-blank line counts as the heading, and only if it is a level 1 heading.
/// An empty heading (`# `) is treated as no heading, which is how `o` detects an aborted create.
fn split_heading(content: &str) -> (Option<String>, &str) {
    let trimmed = content.trim_start();
    let (first_line, rest) = trimmed.split_once('\n').unwrap_or((trimmed, ""));

    // `trim_end` also removes the `\r` of Windows line endings, and turns an empty `# ` into `#`.
    let heading = match first_line.trim_end() {
        "#" => "",
        line => match line.strip_prefix("# ") {
            Some(heading) => heading,
            None => return (None, content),
        },
    };

    let heading = heading.trim();
    let heading = (!heading.is_empty()).then(|| heading.to_string());
    (heading, rest.trim_start_matches(['\r', '\n']))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn task(path: &str, content: &str) -> Task {
        Task::parse(PathBuf::from(path), content)
    }

    #[test]
    fn heading_and_body() {
        let t = task("todo/010-fix.md", "# Fix login bug\n\nsome body\n");
        assert_eq!(t.heading.as_deref(), Some("Fix login bug"));
        assert_eq!(t.body, "some body\n");
        assert_eq!(t.title(), "Fix login bug");
    }

    #[test]
    fn heading_after_blank_lines() {
        let t = task("todo/010-fix.md", "\n\n# Fix\r\nbody");
        assert_eq!(t.heading.as_deref(), Some("Fix"));
        assert_eq!(t.body, "body");
    }

    #[test]
    fn heading_only() {
        let t = task("todo/010-fix.md", "# Fix");
        assert_eq!(t.heading.as_deref(), Some("Fix"));
        assert_eq!(t.body, "");
    }

    #[test]
    fn no_heading_keeps_whole_content() {
        let t = task("todo/010-call-bob.md", "just text\n# Not first\n");
        assert_eq!(t.heading, None);
        assert_eq!(t.body, "just text\n# Not first\n");
        assert_eq!(t.title(), "call bob");
    }

    #[test]
    fn level_two_is_not_a_heading() {
        assert_eq!(task("todo/010-x.md", "## Sub\n").heading, None);
    }

    #[test]
    fn empty_heading_is_none() {
        let t = task("todo/010-x.md", "# \n");
        assert_eq!(t.heading, None);
        assert_eq!(t.body, "");
    }

    #[test]
    fn title_without_prefix() {
        let t = task("todo/notes.md", "");
        assert_eq!(t.name, None);
        assert_eq!(t.title(), "notes");
    }

    #[test]
    fn section_titles() {
        let section = |path: &[&str]| Section {
            path: path.iter().map(|s| s.to_string()).collect(),
            dir: PathBuf::new(),
            tasks: vec![],
        };
        assert_eq!(section(&[]).title(), "inbox");
        assert_eq!(section(&["work"]).title(), "work");
        assert_eq!(section(&["private", "garden"]).title(), "private/garden");
    }
}
