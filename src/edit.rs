//! Turns user actions into [`Op`]s. Decides what should happen, applying is up to the caller.

// Not used yet, the TUI wires this up next.
#![allow(dead_code)]

use crate::{
    name::{Name, order_between, slugify},
    op::Op,
    task::{Section, Task},
};

/// `dd`: removes the task file. The caller keeps the [`Task`] around for pasting.
pub fn cut(task: &Task) -> Vec<Op> {
    vec![Op::Remove {
        path: task.path.clone(),
        content: task.content.clone(),
    }]
}

/// `p` / `P`: writes `task` into `section` so it ends up at position `index`.
///
/// The filename gets an order between its new neighbours and a slug from the title.
/// Returns `None` if there is no free order at that spot (or a neighbour has no prefix),
/// the section has to be renumbered first.
pub fn paste(section: &Section, index: usize, task: &Task) -> Option<Vec<Op>> {
    let order_of = |task: &Task| task.name.as_ref().map(|name| name.order);

    // The `?` inside these returns `None` from `paste` if a neighbour has no prefix.
    let before = match index.checked_sub(1) {
        Some(i) => Some(order_of(&section.tasks[i])?),
        None => None,
    };
    let after = match section.tasks.get(index) {
        Some(task) => Some(order_of(task)?),
        None => None,
    };

    let name = Name {
        order: order_between(before, after)?,
        slug: unique_slug(section, slugify(&task.title())),
    };
    Some(vec![Op::Create {
        path: section.dir.join(format!("{}.md", name.to_stem())),
        content: task.content.clone(),
    }])
}

/// Appends `-2`, `-3`, ... if another task in the section already uses the slug.
fn unique_slug(section: &Section, slug: String) -> String {
    let taken = |candidate: &str| {
        section.tasks.iter().any(|task| {
            task.name
                .as_ref()
                .is_some_and(|name| name.slug == candidate)
        })
    };
    if !taken(&slug) {
        return slug;
    }
    (2..)
        .map(|n| format!("{slug}-{n}"))
        .find(|candidate| !taken(candidate))
        .expect("infinite iterator always finds a free slug")
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::*;

    /// A section in `todo/work` with tasks named by their file names.
    fn section(files: &[&str]) -> Section {
        let dir = PathBuf::from("todo/work");
        Section {
            path: vec!["work".to_string()],
            tasks: files
                .iter()
                .map(|file| Task::parse(dir.join(file), ""))
                .collect(),
            dir,
        }
    }

    fn task(title: &str) -> Task {
        Task::parse(PathBuf::from("todo/old.md"), &format!("# {title}\n\nbody"))
    }

    /// Path of the single file a paste creates.
    fn pasted_path(ops: Option<Vec<Op>>) -> Option<PathBuf> {
        match ops?.as_slice() {
            [Op::Create { path, .. }] => Some(path.clone()),
            other => panic!("expected a single create, got {other:?}"),
        }
    }

    #[test]
    fn cut_removes_with_content() {
        let t = task("Fix bug");
        assert_eq!(
            cut(&t),
            [Op::Remove {
                path: t.path.clone(),
                content: "# Fix bug\n\nbody".to_string()
            }]
        );
    }

    #[test]
    fn paste_keeps_content() {
        let ops = paste(&section(&[]), 0, &task("Fix bug")).unwrap();
        let [Op::Create { content, .. }] = ops.as_slice() else {
            panic!("expected a single create");
        };
        assert_eq!(content, "# Fix bug\n\nbody");
    }

    #[test]
    fn paste_positions() {
        let s = section(&["010-a.md", "020-b.md"]);
        let path = |index| pasted_path(paste(&s, index, &task("New")));

        assert_eq!(path(0), Some(Path::new("todo/work/005-new.md").into()));
        assert_eq!(path(1), Some(Path::new("todo/work/015-new.md").into()));
        assert_eq!(path(2), Some(Path::new("todo/work/030-new.md").into()));
    }

    #[test]
    fn paste_into_empty_section() {
        let path = pasted_path(paste(&section(&[]), 0, &task("New")));
        assert_eq!(path, Some(Path::new("todo/work/010-new.md").into()));
    }

    #[test]
    fn paste_without_gap_needs_renumber() {
        let s = section(&["010-a.md", "011-b.md"]);
        assert_eq!(paste(&s, 1, &task("New")), None);
    }

    #[test]
    fn paste_next_to_unprefixed_needs_renumber() {
        let s = section(&["010-a.md", "notes.md"]);
        assert_eq!(paste(&s, 1, &task("New")), None);
        assert_eq!(paste(&s, 2, &task("New")), None);
        // Before the first task still works, its neighbour has a prefix.
        assert!(paste(&s, 0, &task("New")).is_some());
    }

    #[test]
    fn paste_slug_collision() {
        let s = section(&["010-new.md", "020-new-2.md"]);
        let path = pasted_path(paste(&s, 2, &task("New")));
        assert_eq!(path, Some(Path::new("todo/work/030-new-3.md").into()));
    }
}
