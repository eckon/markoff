//! The order of sections and tasks, kept in `.order` in the root.
//!
//! Not in the filenames: a numbered prefix meant renumbering every file below a change,
//! which made adding a task at the top show up as renames of the whole section in git.
//!
//! The disk decides what exists, the file only orders it: names it does not list go to the end
//! (by name), names it lists that no longer exist are dropped the next time it is written.
//!
//! ```text
//! # work
//! fix-login-bug
//! review-pr
//!
//! # home
//! ```
//!
//! Each section as `# name` with its tasks. Names are file stems (without `.md`) and
//! directory names. Pure logic, reading and writing is up to the caller.

use std::path::PathBuf;

use crate::{op::Op, task::Section};

/// File name in the root. Hidden, so it is never taken for a task.
pub const FILE: &str = ".order";

/// Sections in order, each with its task names in order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Order {
    pub sections: Vec<Group>,
}

/// One section in [`Order`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Group {
    pub name: String,
    pub tasks: Vec<String>,
}

impl Order {
    /// Reads the file content. Never fails: lines that name nothing (typos, git conflict
    /// markers, names before the first heading) simply match no file.
    pub fn parse(content: &str) -> Self {
        let mut order = Self::default();
        for line in content.lines().map(str::trim) {
            if line.is_empty() {
                continue;
            }
            match line.strip_prefix("# ") {
                Some(name) => order.sections.push(Group {
                    name: name.trim().to_string(),
                    tasks: Vec::new(),
                }),
                None => {
                    if let Some(group) = order.sections.last_mut() {
                        group.tasks.push(line.to_string());
                    }
                }
            }
        }
        order
    }

    /// The order as loaded, so it lists exactly what is on disk. Index `s` is `sections[s]`.
    pub fn of(sections: &[Section]) -> Self {
        Self {
            sections: sections
                .iter()
                .map(|section| Group {
                    name: section.title(),
                    tasks: section
                        .tasks
                        .iter()
                        .map(|task| task.stem().to_string())
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
        group.tasks.iter().position(|task| task == stem)
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
                lines.extend(group.tasks.iter().cloned());
                lines.join("\n")
            })
            .collect();
        format!("{}\n", blocks.join("\n\n"))
    }
}

/// The order file as read from disk, `content` is `None` if it does not exist.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrderFile {
    pub path: PathBuf,
    pub content: Option<String>,
}

impl OrderFile {
    /// The op that writes `order`, `None` if the file already says that. No file is created
    /// for an empty order.
    pub fn write(&self, order: &Order) -> Option<Op> {
        let after = order.format();
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
            tasks: tasks.iter().map(|task| task.to_string()).collect(),
        }
    }

    const CONTENT: &str = "# work\nfix-login-bug\nreview-pr\n\n# home\n";

    #[test]
    fn parse_and_format_round_trip() {
        let order = Order::parse(CONTENT);
        assert_eq!(
            order.sections,
            [
                group("work", &["fix-login-bug", "review-pr"]),
                group("home", &[]),
            ]
        );
        assert_eq!(order.format(), CONTENT);
    }

    #[test]
    fn parse_is_forgiving() {
        // Names before the first heading, extra blank lines and spaces, a git conflict.
        let order = Order::parse("x\n\n  # work  \n\n a \n<<<<<<< HEAD\nb\n=======\n");
        assert_eq!(
            order.sections,
            [group("work", &["a", "<<<<<<< HEAD", "b", "======="])]
        );
    }

    #[test]
    fn positions_take_the_first_match() {
        let order = Order::parse("a\n# work\nb\nc\nb\n# home\n# work\nd\n");
        assert_eq!(order.section_position("work"), Some(0));
        assert_eq!(order.section_position("home"), Some(1));
        assert_eq!(order.section_position("nope"), None);
        assert_eq!(order.task_position("", "a"), None, "before any heading");
        assert_eq!(order.task_position("work", "b"), Some(0));
        assert_eq!(order.task_position("work", "c"), Some(1));
        assert_eq!(
            order.task_position("work", "d"),
            None,
            "second `work` is ignored"
        );
        assert_eq!(
            order.task_position("home", "a"),
            None,
            "only its own section"
        );
    }

    #[test]
    fn format_empty() {
        assert_eq!(Order::default().format(), "");
        let only_section = Order {
            sections: vec![group("work", &[])],
        };
        assert_eq!(only_section.format(), "# work\n");
    }

    #[test]
    fn write_creates_updates_or_skips() {
        let order = Order::parse(CONTENT);
        let file = |content: Option<&str>| OrderFile {
            path: PathBuf::from(".order"),
            content: content.map(str::to_string),
        };

        assert_eq!(
            file(None).write(&order),
            Some(Op::Create {
                path: ".order".into(),
                content: CONTENT.to_string()
            })
        );
        assert_eq!(
            file(Some("old\n")).write(&order),
            Some(Op::Update {
                path: ".order".into(),
                before: "old\n".to_string(),
                after: CONTENT.to_string()
            })
        );
        assert_eq!(file(Some(CONTENT)).write(&order), None);
        assert_eq!(file(None).write(&Order::default()), None);
    }
}
