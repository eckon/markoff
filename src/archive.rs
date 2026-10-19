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

/// Level of the section headings: `## work`.
const SECTION_LEVEL: usize = 2;

/// Level of the task headings, one below their section: `# Task` becomes `### Task`.
const TASK_LEVEL: usize = SECTION_LEVEL + 1;

/// How far headings in a task move down, so its `# Task` lands on [`TASK_LEVEL`].
const HEADING_SHIFT: usize = TASK_LEVEL - 1;

/// Adds `task` under `## {section}` and returns the new archive content.
///
/// `archive` is today's file, `None` if it does not exist yet. If the section heading exists,
/// the task goes to the end of that section, otherwise the section is added at the end.
/// An unclosed code fence in an earlier task hides the headings below it, so the section is
/// added again at the end then. Nothing is lost.
pub fn add(archive: Option<&str>, date: &str, section: &str, task: &Task) -> String {
    let entry = entry(task);
    let heading = format!("{} {section}", "#".repeat(SECTION_LEVEL));

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

/// The tasks of the archive file in file order, each with its `## section`: the `### `
/// headings outside code blocks. Tasks above the first section, possible in a file edited by
/// hand, have an empty section. The body is what follows up to the next of these headings, its
/// headings shifted back up as they were in the task.
///
/// A `# ` heading inside a task body is shifted to `### ` as well and counts as a task too. Rare
/// enough for the archive page.
pub fn tasks(archive: &str) -> Vec<Done> {
    let mut fence = Fence::default();
    let mut section = "";
    let mut tasks: Vec<Done> = Vec::new();
    // Lines go to the body of the last task until the next section, the date heading too.
    let mut in_task = false;
    for line in archive.split_inclusive('\n') {
        let level = match fence.update(line) {
            true => None,
            false => heading_level(line),
        };
        match level {
            Some(level) if level <= TASK_LEVEL => {
                in_task = level == TASK_LEVEL;
                match level {
                    SECTION_LEVEL => section = line[SECTION_LEVEL..].trim(),
                    TASK_LEVEL => tasks.push(Done {
                        section: section.to_string(),
                        title: line[TASK_LEVEL..].trim().to_string(),
                        body: String::new(),
                    }),
                    _ => {}
                }
            }
            _ if !in_task => {}
            Some(level) => {
                let body = &mut tasks.last_mut().expect("in a task").body;
                body.push_str(&"#".repeat(level - HEADING_SHIFT));
                body.push_str(&line[level..]);
            }
            None => tasks.last_mut().expect("in a task").body.push_str(line),
        }
    }
    for task in &mut tasks {
        task.body = task.body.trim().to_string();
    }
    tasks
}

/// A task in the archive, see [`tasks`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Done {
    pub section: String,
    pub title: String,
    pub body: String,
}

/// The task as it appears in the archive: `### Title`, then the body with shifted headings.
fn entry(task: &Task) -> String {
    let title = "#".repeat(TASK_LEVEL);
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
        if !fence.update(line) && heading_level(line) == Some(SECTION_LEVEL) {
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
    fn task_bodies_with_their_headings_as_before() {
        let fix = task("# Fix bug\n\n## Notes\n\n```sh\n# comment\n```\n\ndone\n");
        let archive = add(None, "2026-10-14", "work", &fix);
        let archive = add(Some(&archive), "2026-10-14", "home", &task("# B\n"));
        let tasks = tasks(&archive);
        assert_eq!(tasks[0].body, "## Notes\n\n```sh\n# comment\n```\n\ndone");
        assert_eq!(tasks[1].body, "", "the next section ends the body");
    }

    #[test]
    fn tasks_with_their_section() {
        let archive = "### Loose\n\n## work\n\n### A\n\n```\n### no\n## nor\n```\n\n### B\n\n## home\n\n### C\n";
        let tasks: Vec<String> = super::tasks(archive)
            .into_iter()
            .map(|done| format!("{}/{}", done.section, done.title))
            .collect();
        assert_eq!(tasks, ["/Loose", "work/A", "work/B", "home/C"]);
    }

    #[test]
    fn count_tasks() {
        let one = add(None, "2026-10-14", "work", &task("# A\n\n## Sub\n"));
        let two = add(
            Some(&one),
            "2026-10-14",
            "home",
            &task("# B\n\n```\n### no\n```\n"),
        );
        let count = |archive: &str| tasks(archive).len();
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
        let out = add(None, "2026-10-14", "home", &task("# Fix bug\n"));
        assert_eq!(out, "# 2026-10-14\n\n## home\n\n### Fix bug\n");
    }

    #[test]
    fn task_without_heading_uses_filename() {
        let out = add(None, "2026-10-14", "home", &task("just text\n"));
        assert_eq!(out, "# 2026-10-14\n\n## home\n\n### fix bug\n\njust text\n");
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
