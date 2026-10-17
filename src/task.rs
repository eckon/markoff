//! In-memory model of the todo tree: sections that contain tasks.
//!
//! Built from paths and file contents, no file system access here.

use std::path::{Path, PathBuf};

/// One task file, e.g. `work/fix-login-bug.md` in the root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Task {
    pub path: PathBuf,
    /// Text of the `# Heading`, `None` if the file has none.
    pub heading: Option<String>,
    /// Everything after the heading. The whole file if there is no heading.
    pub body: String,
    /// The file exactly as read. Used when the file is removed or written somewhere else.
    pub content: String,
}

impl Task {
    pub fn parse(path: PathBuf, content: &str) -> Self {
        let (heading, body) = split_heading(content);
        Self {
            path,
            heading,
            body: body.to_string(),
            content: content.to_string(),
        }
    }

    /// The heading, or the filename if there is none: `call-bob.md` -> `call bob`.
    pub fn title(&self) -> String {
        match &self.heading {
            Some(heading) => heading.clone(),
            None => self.stem().replace('-', " "),
        }
    }

    /// The filename without `.md`, also its name in the order file.
    pub fn stem(&self) -> &str {
        stem(&self.path).unwrap_or_default()
    }
}

/// A directory of tasks in the root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section {
    /// The directory name, also the display name and the archive heading: `work`.
    pub name: String,
    pub dir: PathBuf,
    pub tasks: Vec<Task>,
}

fn stem(path: &Path) -> Option<&str> {
    path.file_stem()?.to_str()
}

/// Splits `# Title\n\nbody` into `(Some("Title"), "body")`.
///
/// Only the first non-blank line counts as the heading, and only if it is a level 1 heading.
/// An empty heading (`# `) is treated as no heading, which is how `o` / `O` detect an aborted
/// create.
/// No frontmatter support yet: a file starting with `---` has no heading.
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
        let t = task("todo/fix.md", "# Fix login bug\n\nsome body\n");
        assert_eq!(t.heading.as_deref(), Some("Fix login bug"));
        assert_eq!(t.body, "some body\n");
        assert_eq!(t.title(), "Fix login bug");
    }

    #[test]
    fn heading_after_blank_lines() {
        let t = task("todo/fix.md", "\n\n# Fix\r\nbody");
        assert_eq!(t.heading.as_deref(), Some("Fix"));
        assert_eq!(t.body, "body");
    }

    #[test]
    fn heading_only() {
        let t = task("todo/fix.md", "# Fix");
        assert_eq!(t.heading.as_deref(), Some("Fix"));
        assert_eq!(t.body, "");
    }

    #[test]
    fn no_heading_keeps_whole_content() {
        let t = task("todo/call-bob.md", "just text\n# Not first\n");
        assert_eq!(t.heading, None);
        assert_eq!(t.body, "just text\n# Not first\n");
        assert_eq!(t.title(), "call bob");
    }

    #[test]
    fn level_two_is_not_a_heading() {
        assert_eq!(task("todo/x.md", "## Sub\n").heading, None);
    }

    #[test]
    fn empty_heading_is_none() {
        let t = task("todo/x.md", "# \n");
        assert_eq!(t.heading, None);
        assert_eq!(t.body, "");
    }

    #[test]
    fn title_and_stem_from_filename() {
        let t = task("todo/my-notes.md", "");
        assert_eq!(t.title(), "my notes");
        assert_eq!(t.stem(), "my-notes");
    }
}
