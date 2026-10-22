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

use crate::{op::Op, task::Section};

/// File name in the root. Hidden, so it is never taken for a task.
pub const FILE: &str = ".markoff";

/// Sections in order, each with its task names in order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct State {
    pub sections: Vec<Group>,
}

/// One section in [`State`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Group {
    pub name: String,
    pub tasks: Vec<Item>,
}

/// One task in a [`Group`]: `review-pr 2026-10-19`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    pub name: String,
    /// When it was last done.
    pub done: Option<Date>,
}

impl Item {
    /// Names are slugs without spaces, so a date after the last space can only be the date.
    /// Anything else is all name and matches no file.
    fn parse(line: &str) -> Self {
        let dated = (line.rsplit_once(' '))
            .and_then(|(name, date)| Some((name.trim_end(), date.parse().ok()?)));
        let (name, done) = dated.map_or((line, None), |(name, date)| (name, Some(date)));
        Self {
            name: name.to_string(),
            done,
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
                Some(name) => state.sections.push(Group {
                    name: name.trim().to_string(),
                    tasks: Vec::new(),
                }),
                None => {
                    if let Some(group) = state.sections.last_mut() {
                        group.tasks.push(Item::parse(line));
                    }
                }
            }
        }
        state
    }

    /// The order as loaded, so it lists exactly what is on disk. Index `s` is `sections[s]`.
    pub fn of(sections: &[Section]) -> Self {
        Self {
            sections: sections
                .iter()
                .map(|section| Group {
                    name: section.name.clone(),
                    tasks: section
                        .tasks
                        .iter()
                        .map(|task| Item {
                            name: task.stem().to_string(),
                            done: task.done,
                        })
                        .collect(),
                })
                .collect(),
        }
    }

    /// Position of section `name`, first match.
    pub fn section_position(&self, name: &str) -> Option<usize> {
        self.sections.iter().position(|group| group.name == name)
    }

    /// Position of task `stem` in section `section`, first match of both.
    pub fn task_position(&self, section: &str, stem: &str) -> Option<usize> {
        let group = self.sections.iter().find(|group| group.name == section)?;
        group.tasks.iter().position(|task| task.name == stem)
    }

    /// When task `stem` in section `section` was last done, see [`State::task_position`].
    pub fn done(&self, section: &str, stem: &str) -> Option<Date> {
        let group = self.sections.iter().find(|group| group.name == section)?;
        group.tasks.iter().find(|task| task.name == stem)?.done
    }

    /// The file content. Empty if there is nothing to order.
    pub fn format(&self) -> String {
        if self.sections.is_empty() {
            return String::new();
        }
        let blocks: Vec<String> = self
            .sections
            .iter()
            .map(|group| {
                let mut lines = vec![format!("# {}", group.name)];
                lines.extend(group.tasks.iter().map(|task| match task.done {
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

    fn group(name: &str, tasks: &[&str]) -> Group {
        Group {
            name: name.to_string(),
            tasks: tasks.iter().map(|task| Item::parse(task)).collect(),
        }
    }

    const CONTENT: &str = "# work\nfix-login-bug\nreview-pr 2026-10-19\n\n# home\n";

    #[test]
    fn parse_and_format_round_trip() {
        let state = State::parse(CONTENT);
        assert_eq!(
            state.sections,
            [
                group("work", &["fix-login-bug", "review-pr 2026-10-19"]),
                group("home", &[]),
            ]
        );
        assert_eq!(state.format(), CONTENT);
        let date = "2026-10-19".parse().ok();
        assert_eq!(state.sections[0].tasks[1].name, "review-pr");
        assert_eq!(state.sections[0].tasks[1].done, date);
        assert_eq!(state.done("work", "review-pr"), date);
        assert_eq!(state.done("work", "fix-login-bug"), None);
        assert_eq!(state.task_position("work", "review-pr"), Some(1));
    }

    #[test]
    fn parse_only_takes_a_real_date() {
        let state = State::parse("# work\na  2026-10-19\nb 2026-13-01\nc d\n");
        let names: Vec<(&str, Option<Date>)> = (state.sections[0].tasks.iter())
            .map(|task| (task.name.as_str(), task.done))
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
            [group("work", &["a", "<<<<<<< HEAD", "b", "======="])]
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
            sections: vec![group("work", &[])],
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
