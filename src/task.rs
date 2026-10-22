//! In-memory model of the root: sections that contain tasks.
//!
//! Built from paths and file contents, no file system access here.

use std::path::{Path, PathBuf};

use jiff::civil::Date;

use crate::{
    filter,
    repeat::{self, Repeat},
};

/// Extension of task files. Drafts have it too, for markdown highlighting.
pub const EXTENSION: &str = "md";

/// Starts the `# Heading` that is the title, of tasks and of the drafts in the editor.
pub const HEADING: &str = "# ";

/// One task file, e.g. `work/fix-login-bug.md` in the root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Task {
    pub path: PathBuf,
    /// Text of the `# Heading` without a [`Repeat`] rule, `None` if the file has none.
    pub heading: Option<String>,
    /// The rule at the end of the heading, if it repeats.
    pub repeat: Option<Repeat>,
    /// Everything after the heading. The whole file if there is no heading.
    pub body: String,
    /// The file exactly as read. Used when the file is removed or written somewhere else.
    pub content: String,
    /// When it was last done, from the state file, not the task file.
    pub last_done: Option<Date>,
}

impl Task {
    pub fn parse(path: PathBuf, content: &str) -> Self {
        let (heading, body) = split_heading(content);
        // The rule is no part of the title, so the filename and the list leave it out.
        let (heading, repeat) = match heading.as_deref().map(repeat::split) {
            Some((title, repeat)) => (Some(title.to_string()), repeat),
            None => (None, None),
        };
        Self {
            path,
            heading,
            repeat,
            body: body.to_string(),
            content: content.to_string(),
            last_done: None,
        }
    }

    /// The heading, or the filename if there is none: `call-bob.md` -> `call bob`.
    pub fn title(&self) -> String {
        match &self.heading {
            Some(heading) => heading.clone(),
            None => self.stem().replace('-', " "),
        }
    }

    /// Whether the `/` filter `query` is in its title or body.
    pub fn matches(&self, query: &str) -> bool {
        filter::matches(&self.title(), query) || filter::matches(&self.body, query)
    }

    /// A repeating task only while one of its days came, any other task always, see
    /// [`crate::repeat`]. The rest is hidden.
    pub fn due(&self, today: Date) -> bool {
        (self.repeat.as_ref()).is_none_or(|repeat| repeat.due(self.last_done, today))
    }

    /// The filename without `.md`, also its name in the state file.
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

/// The filename without `.md`, `None` if it is no valid UTF-8.
pub fn stem(path: &Path) -> Option<&str> {
    path.file_stem()?.to_str()
}

/// Splits `# Title\n\nbody` into `(Some("Title"), "body")`.
///
/// Only the first non-blank line counts as the heading, and only if it is a level 1 heading.
/// An empty heading (`# `) is treated as no heading, which is how `o` / `O` detect an aborted
/// create.
/// No frontmatter support yet: a file starting with `---` has no heading.
pub fn split_heading(content: &str) -> (Option<String>, &str) {
    let trimmed = content.trim_start();
    let (first_line, rest) = trimmed.split_once('\n').unwrap_or((trimmed, ""));

    // `trim_end` also removes the `\r` of Windows line endings, and turns an empty `# ` into `#`.
    let heading = match first_line.trim_end() {
        line if line == HEADING.trim_end() => "",
        line => match line.strip_prefix(HEADING) {
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
        let t = task("work/fix.md", "# Fix login bug\n\nsome body\n");
        assert_eq!(t.heading.as_deref(), Some("Fix login bug"));
        assert_eq!(t.body, "some body\n");
        assert_eq!(t.title(), "Fix login bug");
    }

    #[test]
    fn repeat_rule_is_no_part_of_the_title() {
        let t = task("work/water.md", "# Water plants `* * mon`\n");
        assert_eq!(t.title(), "Water plants");
        assert_eq!(t.repeat.unwrap().rule, "* * mon");

        let t = task("work/run.md", "# Run `cargo test`\n");
        assert_eq!(t.title(), "Run `cargo test`", "no rule");
        assert_eq!(t.repeat, None);
    }

    #[test]
    fn heading_after_blank_lines() {
        let t = task("work/fix.md", "\n\n# Fix\r\nbody");
        assert_eq!(t.heading.as_deref(), Some("Fix"));
        assert_eq!(t.body, "body");
    }

    #[test]
    fn heading_only() {
        let t = task("work/fix.md", "# Fix");
        assert_eq!(t.heading.as_deref(), Some("Fix"));
        assert_eq!(t.body, "");
    }

    #[test]
    fn no_heading_keeps_whole_content() {
        let t = task("work/call-bob.md", "just text\n# Not first\n");
        assert_eq!(t.heading, None);
        assert_eq!(t.body, "just text\n# Not first\n");
        assert_eq!(t.title(), "call bob");
    }

    #[test]
    fn level_two_is_not_a_heading() {
        assert_eq!(task("work/x.md", "## Sub\n").heading, None);
    }

    #[test]
    fn empty_heading_is_none() {
        let t = task("work/x.md", "# \n");
        assert_eq!(t.heading, None);
        assert_eq!(t.body, "");
    }

    #[test]
    fn title_and_stem_from_filename() {
        let t = task("work/my-notes.md", "");
        assert_eq!(t.title(), "my notes");
        assert_eq!(t.stem(), "my-notes");
    }
}
