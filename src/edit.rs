//! Turns user actions into [`Op`]s. Decides what should happen, applying is up to the caller.

use std::path::{Path, PathBuf};

use crate::{
    archive,
    name::{MAX_ORDER, Name, ORDER_GAP, order_between, slugify},
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

/// `x`: adds the task to today's archive file, then removes it.
///
/// `existing` is the current content of `archive_path`, `None` if the file does not exist.
/// Archive first: if removing the task fails, the rollback takes the entry out again.
pub fn archive(
    task: &Task,
    section: &Section,
    archive_path: &Path,
    existing: Option<&str>,
    date: &str,
) -> Vec<Op> {
    let after = archive::add(existing, date, &section.title(), task);
    let path = archive_path.to_path_buf();
    let write = match existing {
        Some(before) => Op::Update {
            path,
            before: before.to_string(),
            after,
        },
        None => Op::Create {
            path,
            content: after,
        },
    };

    let mut ops = vec![write];
    ops.extend(cut(task));
    ops
}

/// Renames the tasks of a section into the shape markoff itself creates.
///
/// Tasks without prefix get one after the last task, in the order they are shown. If that
/// does not fit into 999, the whole section is renumbered. Slugs follow the heading, tasks
/// without heading keep their name. Returns no ops if everything already matches.
pub fn normalize(section: &Section) -> Vec<Op> {
    let Some(orders) = normalized_orders(section) else {
        return Vec::new();
    };
    let names = target_names(section, &orders);
    renames_to(section, &names)
}

/// Prefixed tasks keep their order, the others are appended [`ORDER_GAP`] apart.
/// If that does not fit, all tasks get even gaps. `None` if the section is too big even for that.
fn normalized_orders(section: &Section) -> Option<Vec<u32>> {
    let last = section
        .tasks
        .iter()
        .filter_map(|task| task.name.as_ref().map(|name| name.order))
        .max()
        .unwrap_or(0);
    let mut next = last;
    let appended: Option<Vec<u32>> = section
        .tasks
        .iter()
        .map(|task| match &task.name {
            Some(name) => Some(name.order),
            None => {
                next += ORDER_GAP;
                (next <= MAX_ORDER).then_some(next)
            }
        })
        .collect();

    appended.or_else(|| {
        let gap = gap_for(section.tasks.len())?;
        Some(
            (1..=section.tasks.len() as u32)
                .map(|position| position * gap)
                .collect(),
        )
    })
}

/// Names for all tasks at `orders`: slug from the heading, otherwise the current name.
///
/// Tasks whose slug stays the same keep it, the others get a unique one (`-2`, ...), so a
/// task never takes the slug another task already has.
fn target_names(section: &Section, orders: &[u32]) -> Vec<Name> {
    let slugs: Vec<String> = section.tasks.iter().map(wanted_slug).collect();
    let keeps_slug: Vec<bool> = section
        .tasks
        .iter()
        .zip(&slugs)
        .map(|(task, slug)| task.name.as_ref().map(|name| &name.slug) == Some(slug))
        .collect();

    let mut taken: Vec<Name> = (0..slugs.len())
        .filter(|&i| keeps_slug[i])
        .map(|i| Name {
            order: orders[i],
            slug: slugs[i].clone(),
        })
        .collect();

    slugs
        .into_iter()
        .zip(orders)
        .enumerate()
        .map(|(i, (slug, &order))| {
            if keeps_slug[i] {
                return Name { order, slug };
            }
            let name = Name {
                order,
                slug: unique_slug(&taken, slug),
            };
            taken.push(name.clone());
            name
        })
        .collect()
}

/// Slug from the heading. Without heading the name stays: the slug, or the whole file stem
/// if there is no prefix (`My Notes.md` -> `010-My Notes.md`).
fn wanted_slug(task: &Task) -> String {
    match (&task.heading, &task.name) {
        (Some(heading), _) => slugify(heading),
        (None, Some(name)) => name.slug.clone(),
        (None, None) => task
            .path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or_default()
            .to_string(),
    }
}

/// Renames for every task whose file name differs from `names`, ordered to avoid clashes.
fn renames_to(section: &Section, names: &[Name]) -> Vec<Op> {
    let renames = section
        .tasks
        .iter()
        .zip(names)
        .map(|(task, name)| {
            let to = section.dir.join(format!("{}.md", name.to_stem()));
            (task.path.clone(), to)
        })
        .filter(|(from, to)| from != to)
        .collect();
    order_renames(renames)
}

/// `p` / `P`: writes `task` into `section` so it ends up at position `index`.
///
/// The filename gets an order between its new neighbours and a slug from the title.
/// If there is no free order at that spot, the section is renumbered first (same op list,
/// so one undo reverts both). Returns `None` only if the section is full.
pub fn paste(section: &Section, index: usize, task: &Task) -> Option<Vec<Op>> {
    let (mut ops, names, order) = match free_order(section, index) {
        Some(order) => {
            let names = section
                .tasks
                .iter()
                .filter_map(|t| t.name.clone())
                .collect();
            (Vec::new(), names, order)
        }
        None => renumber(section, index)?,
    };

    let name = Name {
        order,
        slug: unique_slug(&names, slugify(&task.title())),
    };
    ops.push(Op::Create {
        path: section.dir.join(format!("{}.md", name.to_stem())),
        content: task.content.clone(),
    });
    Some(ops)
}

/// An unused order between the tasks around `index`.
///
/// `None` if there is no gap, or if a neighbour has no prefix (then there is nothing
/// to place the new task relative to).
fn free_order(section: &Section, index: usize) -> Option<u32> {
    let order_of = |task: &Task| task.name.as_ref().map(|name| name.order);

    // The `?` inside these returns `None` from `free_order` if a neighbour has no prefix.
    let before = match index.checked_sub(1) {
        Some(i) => Some(order_of(&section.tasks[i])?),
        None => None,
    };
    let after = match section.tasks.get(index) {
        Some(task) => Some(order_of(task)?),
        None => None,
    };
    order_between(before, after)
}

/// Renumbers all tasks with even gaps, leaving position `slot` free for a new task.
///
/// Returns the renames, the new names of all tasks and the order for the free slot.
fn renumber(section: &Section, slot: usize) -> Option<(Vec<Op>, Vec<Name>, u32)> {
    let gap = gap_for(section.tasks.len() + 1)?;
    let order_at = |position: usize| (position as u32 + 1) * gap;
    let orders: Vec<u32> = (0..section.tasks.len())
        .map(|i| order_at(if i < slot { i } else { i + 1 }))
        .collect();

    let names = target_names(section, &orders);
    Some((renames_to(section, &names), names, order_at(slot)))
}

/// The largest of 10, 5, 2, 1 that fits `count` tasks into 999.
fn gap_for(count: usize) -> Option<u32> {
    [ORDER_GAP, 5, 2, 1]
        .into_iter()
        .find(|gap| count as u32 * gap <= MAX_ORDER)
}

/// Orders renames so each target is free when its turn comes.
///
/// `009-x -> 010-x` has to wait until the old `010-x` has moved on. Should no order exist,
/// the rest is kept as is: [`Op::Rename`] refuses to overwrite, so that ends in an error
/// and a rollback, never in a lost file.
fn order_renames(mut pending: Vec<(PathBuf, PathBuf)>) -> Vec<Op> {
    let mut ordered = Vec::new();
    while !pending.is_empty() {
        let is_free = |to: &PathBuf| !pending.iter().any(|(from, _)| from == to);
        let next = pending.iter().position(|(_, to)| is_free(to)).unwrap_or(0);
        let (from, to) = pending.remove(next);
        ordered.push(Op::Rename { from, to });
    }
    ordered
}

/// Appends `-2`, `-3`, ... if another task in the section already uses the slug.
fn unique_slug(names: &[Name], slug: String) -> String {
    let taken = |candidate: &str| names.iter().any(|name| name.slug == candidate);
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

    /// Path of the file a paste creates, the last op.
    fn pasted_path(ops: Option<Vec<Op>>) -> Option<PathBuf> {
        match ops?.last() {
            Some(Op::Create { path, .. }) => Some(path.clone()),
            other => panic!("expected a create at the end, got {other:?}"),
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
    fn archive_creates_or_updates_then_removes() {
        let s = section(&["010-fix-bug.md"]);
        let t = &s.tasks[0];
        let path = Path::new("archive/2026-10-14.md");

        let ops = archive(t, &s, path, None, "2026-10-14");
        assert_eq!(
            describe(Some(ops)).unwrap(),
            ["create 2026-10-14.md", "remove 010-fix-bug.md"]
        );

        let ops = archive(t, &s, path, Some("# 2026-10-14\n"), "2026-10-14");
        assert_eq!(
            describe(Some(ops)).unwrap(),
            ["update 2026-10-14.md", "remove 010-fix-bug.md"]
        );
    }

    /// A section in `todo/work` from `(file, content)` pairs.
    fn section_with(files: &[(&str, &str)]) -> Section {
        let mut s = section(&[]);
        s.tasks = files
            .iter()
            .map(|(file, content)| Task::parse(s.dir.join(file), content))
            .collect();
        s
    }

    #[test]
    fn normalize_leaves_clean_section_alone() {
        let s = section_with(&[("010-a.md", "# A"), ("020-notes.md", "no heading")]);
        assert_eq!(normalize(&s), []);
    }

    #[test]
    fn normalize_syncs_slug_to_heading() {
        let s = section_with(&[("010-old.md", "# New title")]);
        assert_eq!(
            describe(Some(normalize(&s))).unwrap(),
            ["rename 010-old.md -> 010-new-title.md"]
        );
    }

    #[test]
    fn normalize_appends_unprefixed_in_shown_order() {
        // Loaded order: prefixed first, then unprefixed by name.
        let s = section_with(&[
            ("010-a.md", "# A"),
            ("Call Bob.md", "no heading"),
            ("random.md", "# Fix login"),
        ]);
        assert_eq!(
            describe(Some(normalize(&s))).unwrap(),
            [
                "rename Call Bob.md -> 020-Call Bob.md",
                "rename random.md -> 030-fix-login.md",
            ]
        );
    }

    #[test]
    fn normalize_never_takes_a_used_slug() {
        // `010-x-2` keeps its `-2` because `020-x` already has `x`.
        let s = section_with(&[
            ("010-x-2.md", "# X"),
            ("020-x.md", "# X"),
            ("030-old.md", "# X"),
        ]);
        assert_eq!(
            describe(Some(normalize(&s))).unwrap(),
            ["rename 030-old.md -> 030-x-3.md"]
        );
    }

    #[test]
    fn normalize_renumbers_if_appending_does_not_fit() {
        let s = section_with(&[("995-a.md", "# A"), ("b.md", "# B")]);
        assert_eq!(
            describe(Some(normalize(&s))).unwrap(),
            ["rename 995-a.md -> 010-a.md", "rename b.md -> 020-b.md"]
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

    /// Ops as short strings with file names only: `rename 011-b.md -> 030-b.md`.
    fn describe(ops: Option<Vec<Op>>) -> Option<Vec<String>> {
        let file = |path: &Path| path.file_name().unwrap().to_str().unwrap().to_string();
        let ops = ops?
            .iter()
            .map(|op| match op {
                Op::Create { path, .. } => format!("create {}", file(path)),
                Op::Remove { path, .. } => format!("remove {}", file(path)),
                Op::Rename { from, to } => format!("rename {} -> {}", file(from), file(to)),
                Op::Update { path, .. } => format!("update {}", file(path)),
            })
            .collect();
        Some(ops)
    }

    #[test]
    fn paste_without_gap_renumbers() {
        let s = section(&["010-a.md", "011-b.md", "012-c.md"]);
        assert_eq!(
            describe(paste(&s, 1, &task("New"))).unwrap(),
            [
                "rename 011-b.md -> 030-b.md",
                "rename 012-c.md -> 040-c.md",
                "create 020-new.md",
            ]
        );
    }

    #[test]
    fn paste_next_to_unprefixed_renumbers() {
        let s = section(&["010-a.md", "My Notes.md"]);
        assert_eq!(
            describe(paste(&s, 2, &task("New"))).unwrap(),
            ["rename My Notes.md -> 020-My Notes.md", "create 030-new.md"]
        );
        // Before the first task needs no renumber, its neighbour has a prefix.
        assert_eq!(
            describe(paste(&s, 0, &task("New"))).unwrap(),
            ["create 005-new.md"]
        );
    }

    #[test]
    fn renames_are_ordered_to_avoid_clashes() {
        // `009-x` becomes `010-x`, which is free only after the old `010-x` moved away.
        let s = section(&["009-x.md", "010-x.md"]);
        assert_eq!(
            describe(paste(&s, 1, &task("New"))).unwrap(),
            [
                "rename 010-x.md -> 030-x.md",
                "rename 009-x.md -> 010-x.md",
                "create 020-new.md",
            ]
        );
    }

    #[test]
    fn renumber_shrinks_gap_for_big_sections() {
        let files: Vec<String> = (1..=100).map(|i| format!("{i:03}-t{i}.md")).collect();
        let s = section(&files.iter().map(String::as_str).collect::<Vec<_>>());

        // 101 tasks with the new one: gap 10 would need 1010, gap 5 fits (505).
        let ops = describe(paste(&s, 1, &task("New"))).unwrap();
        assert!(ops.contains(&"create 010-new.md".to_string()));
        assert!(ops.contains(&"rename 100-t100.md -> 505-t100.md".to_string()));
    }

    #[test]
    fn full_section_cannot_paste() {
        let files: Vec<String> = (1..=999).map(|i| format!("{i:03}-t{i}.md")).collect();
        let s = section(&files.iter().map(String::as_str).collect::<Vec<_>>());
        assert_eq!(paste(&s, 1, &task("New")), None);
    }

    #[test]
    fn paste_slug_collision() {
        let s = section(&["010-new.md", "020-new-2.md"]);
        let path = pasted_path(paste(&s, 2, &task("New")));
        assert_eq!(path, Some(Path::new("todo/work/030-new-3.md").into()));
    }
}
