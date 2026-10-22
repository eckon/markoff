//! markoff's own state, kept in `.markoff` in the root apart from the task files, so it is not
//! versioned with them: the order of sections and tasks, and when a task was last done.
//! Unlisted names go to the end, stale ones are dropped on the next write. The date is on the
//! task's line, so it goes wherever the line goes.
//!
//! ```text
//! # work
//! fix-login-bug
//! review-pr 2026-10-19
//!
//! # home
//! ```
//!
//! Names are directory names and file stems. Pure logic, reading and writing is up to the caller.

use std::path::PathBuf;

use jiff::civil::Date;

use crate::{op::Op, task};

/// File name in the root. Hidden, so it is never taken for a task.
pub const FILE: &str = ".markoff";

/// Sections in order, each with its task names in order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct State {
    pub sections: Vec<Section>,
}

/// One section in [`State`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section {
    pub name: String,
    pub tasks: Vec<Task>,
}

/// One task in a [`Section`]: `review-pr 2026-10-19`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Task {
    pub name: String,
    /// When it was last done.
    pub last_done: Option<Date>,
}

impl Task {
    /// Names are slugs without spaces, so a date after the last space can only be the date.
    /// Anything else is all name and matches no file.
    fn parse(line: &str) -> Self {
        let dated = (line.rsplit_once(' '))
            .and_then(|(name, date)| Some((name.trim_end(), date.parse().ok()?)));
        let (name, last_done) = dated.map_or((line, None), |(name, date)| (name, Some(date)));
        Self {
            name: name.to_string(),
            last_done,
        }
    }
}

impl State {
    /// Reads the file content. Never fails: lines that name nothing (typos, git conflict
    /// markers, names before the first heading) simply match no file.
    pub fn parse(content: &str) -> Self {
        let mut state = Self::default();
        for line in content.lines().map(str::trim) {
            if line.is_empty() {
                continue;
            }
            match line.strip_prefix("# ") {
                Some(name) => state.sections.push(Section {
                    name: name.trim().to_string(),
                    tasks: Vec::new(),
                }),
                None => {
                    if let Some(section) = state.sections.last_mut() {
                        section.tasks.push(Task::parse(line));
                    }
                }
            }
        }
        state
    }

    /// The order as loaded, so it lists exactly what is on disk. Index `s` is `sections[s]`.
    pub fn of(sections: &[task::Section]) -> Self {
        Self {
            sections: sections
                .iter()
                .map(|section| Section {
                    name: section.name.clone(),
                    tasks: section
                        .tasks
                        .iter()
                        .map(|task| Task {
                            name: task.stem().to_string(),
                            last_done: task.last_done,
                        })
                        .collect(),
                })
                .collect(),
        }
    }

    /// Position of section `name`, first match.
    pub fn section_position(&self, name: &str) -> Option<usize> {
        self.sections
            .iter()
            .position(|section| section.name == name)
    }

    /// Position of task `stem` in section `section`, first match of both.
    pub fn task_position(&self, section: &str, stem: &str) -> Option<usize> {
        let order = self.sections.iter().find(|s| s.name == section)?;
        order.tasks.iter().position(|task| task.name == stem)
    }

    /// When task `stem` in section `section` was last done, see [`State::task_position`].
    pub fn last_done(&self, section: &str, stem: &str) -> Option<Date> {
        let order = self.sections.iter().find(|s| s.name == section)?;
        order.tasks.iter().find(|task| task.name == stem)?.last_done
    }

    /// The file content. Empty if there is nothing to order.
    pub fn format(&self) -> String {
        if self.sections.is_empty() {
            return String::new();
        }
        let blocks: Vec<String> = self
            .sections
            .iter()
            .map(|section| {
                let mut lines = vec![format!("# {}", section.name)];
                lines.extend(section.tasks.iter().map(|task| match task.last_done {
                    Some(done) => format!("{} {done}", task.name),
                    None => task.name.clone(),
                }));
                lines.join("\n")
            })
            .collect();
        format!("{}\n", blocks.join("\n\n"))
    }
}

/// The state file as read from disk, `content` is `None` if it does not exist.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StateFile {
    pub path: PathBuf,
    pub content: Option<String>,
}

impl StateFile {
    /// The op that writes `state`, `None` if the file already says that. No file is created
    /// for an empty order.
    pub fn write(&self, state: &State) -> Option<Op> {
        let after = state.format();
        let path = self.path.clone();
        match &self.content {
            Some(before) if *before == after => None,
            None if after.is_empty() => None,
            Some(before) => Some(Op::Update {
                path,
                before: before.clone(),
                after,
            }),
            None => Some(Op::Create {
                path,
                content: after,
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn section(name: &str, tasks: &[&str]) -> Section {
        Section {
            name: name.to_string(),
            tasks: tasks.iter().map(|task| Task::parse(task)).collect(),
        }
    }

    const CONTENT: &str = "# work\nfix-login-bug\nreview-pr 2026-10-19\n\n# home\n";

    #[test]
    fn parse_and_format_round_trip() {
        let state = State::parse(CONTENT);
        assert_eq!(
            state.sections,
            [
                section("work", &["fix-login-bug", "review-pr 2026-10-19"]),
                section("home", &[]),
            ]
        );
        assert_eq!(state.format(), CONTENT);
        let date = "2026-10-19".parse().ok();
        assert_eq!(state.sections[0].tasks[1].name, "review-pr");
        assert_eq!(state.sections[0].tasks[1].last_done, date);
        assert_eq!(state.last_done("work", "review-pr"), date);
        assert_eq!(state.last_done("work", "fix-login-bug"), None);
        assert_eq!(state.task_position("work", "review-pr"), Some(1));
    }

    #[test]
    fn parse_only_takes_a_real_date() {
        let state = State::parse("# work\na  2026-10-19\nb 2026-13-01\nc d\n");
        let names: Vec<(&str, Option<Date>)> = (state.sections[0].tasks.iter())
            .map(|task| (task.name.as_str(), task.last_done))
            .collect();
        let date = "2026-10-19".parse().ok();
        assert_eq!(
            names,
            [("a", date), ("b 2026-13-01", None), ("c d", None)],
            "anything else matches no file"
        );
    }

    #[test]
    fn parse_is_forgiving() {
        // Names before the first heading, extra blank lines and spaces, a git conflict.
        let state = State::parse("x\n\n  # work  \n\n a \n<<<<<<< HEAD\nb\n=======\n");
        assert_eq!(
            state.sections,
            [section("work", &["a", "<<<<<<< HEAD", "b", "======="])]
        );
    }

    #[test]
    fn positions_take_the_first_match() {
        let state = State::parse("a\n# work\nb\nc\nb\n# home\n# work\nd\n");
        assert_eq!(state.section_position("work"), Some(0));
        assert_eq!(state.section_position("home"), Some(1));
        assert_eq!(state.section_position("nope"), None);
        assert_eq!(state.task_position("", "a"), None, "before any heading");
        assert_eq!(state.task_position("work", "b"), Some(0));
        assert_eq!(state.task_position("work", "c"), Some(1));
        assert_eq!(
            state.task_position("work", "d"),
            None,
            "second `work` is ignored"
        );
        assert_eq!(
            state.task_position("home", "a"),
            None,
            "only its own section"
        );
    }

    #[test]
    fn format_empty() {
        assert_eq!(State::default().format(), "");
        let only_section = State {
            sections: vec![section("work", &[])],
        };
        assert_eq!(only_section.format(), "# work\n");
    }

    #[test]
    fn write_creates_updates_or_skips() {
        let state = State::parse(CONTENT);
        let file = |content: Option<&str>| StateFile {
            path: PathBuf::from(".markoff"),
            content: content.map(str::to_string),
        };

        assert_eq!(
            file(None).write(&state),
            Some(Op::Create {
                path: ".markoff".into(),
                content: CONTENT.to_string()
            })
        );
        assert_eq!(
            file(Some("old\n")).write(&state),
            Some(Op::Update {
                path: ".markoff".into(),
                before: "old\n".to_string(),
                after: CONTENT.to_string()
            })
        );
        assert_eq!(file(Some(CONTENT)).write(&state), None);
        assert_eq!(file(None).write(&State::default()), None);
    }
}
