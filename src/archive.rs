//! Text of the daily archive file, `.archive/YYYY-MM-DD.md` in the root.
//!
//! ```markdown
//! # 2026-10-14
//!
//! ## work
//!
//! ### Fix login bug
//!
//! body of the task
//! ```
//!
//! Pure string handling, writing the file is up to the caller.

use crate::task::Task;

/// Directory in the root. Hidden, so it is never loaded as a section.
pub const DIR: &str = ".archive";

/// Deepest heading level in markdown.
const MAX_HEADING_LEVEL: usize = 6;

/// How far task headings move down: `# Task` becomes `### Task` below `## section`.
const HEADING_SHIFT: usize = 2;

/// Adds `task` under `## {section}` and returns the new archive content.
///
/// `archive` is today's file, `None` if it does not exist yet. If the section heading exists,
/// the task goes to the end of that section, otherwise the section is added at the end.
/// An unclosed code fence in an earlier task hides the headings below it, so the section is
/// added again at the end then. Nothing is lost.
pub fn add(archive: Option<&str>, date: &str, section: &str, task: &Task) -> String {
    let entry = entry(task);
    let heading = format!("## {section}");

    let Some(archive) = archive else {
        return format!("# {date}\n\n{heading}\n\n{entry}\n");
    };

    match section_end(archive, &heading) {
        Some(end) => {
            let (before, after) = archive.split_at(end);
            let mut out = format!("{}\n\n{entry}\n", before.trim_end());
            if !after.is_empty() {
                out.push('\n');
                out.push_str(after);
            }
            out
        }
        None => format!("{}\n\n{heading}\n\n{entry}\n", archive.trim_end()),
    }
}

/// How many tasks the archive file holds: its `### ` headings outside code blocks.
///
/// A `# ` heading inside a task body is shifted to `### ` as well and counts too. Rare enough
/// for a number in the status line.
pub fn count(archive: &str) -> usize {
    let mut fence = Fence::default();
    archive
        .split_inclusive('\n')
        .filter(|line| !fence.update(line) && heading_level(line) == Some(1 + HEADING_SHIFT))
        .count()
}

/// The task as it appears in the archive: `### Title`, then the body with shifted headings.
fn entry(task: &Task) -> String {
    let title = "#".repeat(1 + HEADING_SHIFT);
    let body = shift_headings(task.body.trim());
    if body.is_empty() {
        format!("{title} {}", task.title())
    } else {
        format!("{title} {}\n\n{body}", task.title())
    }
}

/// Moves every heading [`HEADING_SHIFT`] levels down, capped at [`MAX_HEADING_LEVEL`].
/// Lines in fenced code blocks are left alone (`# comment` in a shell snippet).
fn shift_headings(body: &str) -> String {
    let mut fence = Fence::default();
    let mut out = String::new();
    for line in body.split_inclusive('\n') {
        if !fence.update(line)
            && let Some(level) = heading_level(line)
        {
            let new_level = (level + HEADING_SHIFT).min(MAX_HEADING_LEVEL);
            out.push_str(&"#".repeat(new_level));
            out.push_str(&line[level..]);
        } else {
            out.push_str(line);
        }
    }
    out
}

/// Byte offset where the section with `heading` ends: the start of the next `## ` line
/// or the end of the file. `None` if the section does not exist.
fn section_end(archive: &str, heading: &str) -> Option<usize> {
    let mut fence = Fence::default();
    let mut in_section = false;
    let mut offset = 0;
    for line in archive.split_inclusive('\n') {
        if !fence.update(line) && heading_level(line) == Some(2) {
            if in_section {
                return Some(offset);
            }
            in_section = line.trim_end() == heading;
        }
        offset += line.len();
    }
    in_section.then_some(archive.len())
}

/// Level of an ATX heading (`## Title` -> 2), `None` for any other line.
fn heading_level(line: &str) -> Option<usize> {
    let level = line.bytes().take_while(|&b| b == b'#').count();
    let rest = &line[level..];
    let is_heading = (1..=MAX_HEADING_LEVEL).contains(&level)
        && (rest.is_empty() || rest.starts_with([' ', '\t', '\r', '\n']));
    is_heading.then_some(level)
}

/// Tracks whether we are inside a fenced code block (``` or ~~~).
#[derive(Default)]
struct Fence {
    open: Option<&'static str>,
}

impl Fence {
    /// Feeds the next line, returns `true` if it is part of a code block (fences included).
    fn update(&mut self, line: &str) -> bool {
        let trimmed = line.trim_start();
        let marker = ["```", "~~~"].into_iter().find(|m| trimmed.starts_with(m));
        match (self.open, marker) {
            (None, Some(marker)) => {
                self.open = Some(marker);
                true
            }
            (Some(open), Some(marker)) if open == marker => {
                self.open = None;
                true
            }
            (open, _) => open.is_some(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn task(content: &str) -> Task {
        Task::parse(PathBuf::from("todo/fix-bug.md"), content)
    }

    #[test]
    fn count_tasks() {
        let one = add(None, "2026-10-14", "work", &task("# A\n\n## Sub\n"));
        let two = add(
            Some(&one),
            "2026-10-14",
            "inbox",
            &task("# B\n\n```\n### no\n```\n"),
        );
        assert_eq!(count(""), 0);
        assert_eq!(count(&one), 1);
        assert_eq!(count(&two), 2);
    }

    #[test]
    fn new_archive() {
        let out = add(None, "2026-10-14", "work", &task("# Fix bug\n\nbody\n"));
        assert_eq!(out, "# 2026-10-14\n\n## work\n\n### Fix bug\n\nbody\n");
    }

    #[test]
    fn task_without_body() {
        let out = add(None, "2026-10-14", "inbox", &task("# Fix bug\n"));
        assert_eq!(out, "# 2026-10-14\n\n## inbox\n\n### Fix bug\n");
    }

    #[test]
    fn task_without_heading_uses_filename() {
        let out = add(None, "2026-10-14", "inbox", &task("just text\n"));
        assert_eq!(
            out,
            "# 2026-10-14\n\n## inbox\n\n### fix bug\n\njust text\n"
        );
    }

    #[test]
    fn new_section_goes_to_the_end() {
        let archive = "# 2026-10-14\n\n## work\n\n### A\n";
        let out = add(Some(archive), "2026-10-14", "private", &task("# B\n"));
        assert_eq!(
            out,
            "# 2026-10-14\n\n## work\n\n### A\n\n## private\n\n### B\n"
        );
    }

    #[test]
    fn existing_section_gets_the_task_at_its_end() {
        let archive = "# 2026-10-14\n\n## work\n\n### A\n\n## private\n\n### B\n";
        let out = add(Some(archive), "2026-10-14", "work", &task("# C\n"));
        assert_eq!(
            out,
            "# 2026-10-14\n\n## work\n\n### A\n\n### C\n\n## private\n\n### B\n"
        );
    }

    #[test]
    fn existing_last_section() {
        let archive = "# 2026-10-14\n\n## work\n\n### A\n";
        let out = add(Some(archive), "2026-10-14", "work", &task("# C\n"));
        assert_eq!(out, "# 2026-10-14\n\n## work\n\n### A\n\n### C\n");
    }

    #[test]
    fn headings_in_body_are_shifted_and_capped() {
        let out = shift_headings("# One\n## Two\n##### Five\nnot#heading\n#nospace\n");
        assert_eq!(
            out,
            "### One\n#### Two\n###### Five\nnot#heading\n#nospace\n"
        );
    }

    #[test]
    fn code_blocks_are_left_alone() {
        let body = "## Notes\n```sh\n# comment\n## also\n```\n# After\n";
        assert_eq!(
            shift_headings(body),
            "#### Notes\n```sh\n# comment\n## also\n```\n### After\n"
        );
    }

    #[test]
    fn section_heading_inside_code_block_is_ignored() {
        let archive = "# d\n\n## work\n\n### A\n\n```\n## private\n```\n\n## private\n\n### B\n";
        let out = add(Some(archive), "d", "private", &task("# C\n"));
        assert!(
            out.ends_with("## private\n\n### B\n\n### C\n"),
            "got:\n{out}"
        );
    }
}
