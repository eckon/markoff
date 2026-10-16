//! Turns user actions into [`Op`]s. Decides what should happen, applying is up to the caller.

use std::path::{Path, PathBuf};

use crate::{
    archive,
    name::{MAX_ORDER, Name, ORDER_GAP, order_between, slugify},
    op::Op,
    task::{Section, Task},
};

/// `D`: removes the task file, the content is kept in the op for undo.
pub fn delete(task: &Task) -> Vec<Op> {
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
    ops.extend(delete(task));
    ops
}

/// A task file or section directory, as far as naming and order are concerned.
struct Entry {
    path: PathBuf,
    /// `None` without a valid `NNN-` prefix.
    name: Option<Name>,
    /// The slug it should have: from the heading for tasks, the current one otherwise.
    slug: String,
    /// `.md` for task files, empty for directories.
    suffix: &'static str,
}

/// The tasks of a section as [`Entry`]s, in display order.
fn task_entries(section: &Section) -> Vec<Entry> {
    section
        .tasks
        .iter()
        .map(|task| Entry {
            path: task.path.clone(),
            name: task.name.clone(),
            slug: wanted_slug(task),
            suffix: ".md",
        })
        .collect()
}

/// The section directories as [`Entry`]s, in display order. The inbox is not one of them.
fn section_entries(sections: &[Section]) -> Vec<Entry> {
    sections[1..]
        .iter()
        .map(|section| {
            let dir_name = section
                .dir
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_default();
            let name = Name::parse(dir_name);
            let slug = name
                .as_ref()
                .map_or_else(|| dir_name.to_string(), |name| name.slug.clone());
            Entry {
                path: section.dir.clone(),
                name,
                slug,
                suffix: "",
            }
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

/// Renames the tasks of a section into the shape markoff itself creates.
///
/// Tasks without prefix get one after the last task, in the order they are shown. If that
/// does not fit into 999, the whole section is renumbered. Slugs follow the heading, tasks
/// without heading keep their name. Returns no ops if everything already matches.
pub fn normalize(section: &Section) -> Vec<Op> {
    let entries = task_entries(section);
    let Some(orders) = normalized_orders(&entries) else {
        return Vec::new();
    };
    let names = target_names(&entries, &orders);
    renames_to(&entries, &section.dir, &names)
}

/// Prefixed entries keep their order, the others are appended [`ORDER_GAP`] apart.
/// If that does not fit, all get even gaps. `None` if there are too many even for that.
fn normalized_orders(entries: &[Entry]) -> Option<Vec<u32>> {
    let last = entries
        .iter()
        .filter_map(|entry| entry.name.as_ref().map(|name| name.order))
        .max()
        .unwrap_or(0);
    let mut next = last;
    let appended: Option<Vec<u32>> = entries
        .iter()
        .map(|entry| match &entry.name {
            Some(name) => Some(name.order),
            None => {
                next += ORDER_GAP;
                (next <= MAX_ORDER).then_some(next)
            }
        })
        .collect();

    appended.or_else(|| {
        let gap = gap_for(entries.len())?;
        Some(
            (1..=entries.len() as u32)
                .map(|position| position * gap)
                .collect(),
        )
    })
}

/// Names for all entries at `orders`, with their wanted slug.
///
/// Entries whose slug stays the same keep it, the others get a unique one (`-2`, ...), so
/// an entry never takes the slug another one already has.
fn target_names(entries: &[Entry], orders: &[u32]) -> Vec<Name> {
    let keeps_slug: Vec<bool> = entries
        .iter()
        .map(|entry| entry.name.as_ref().map(|name| &name.slug) == Some(&entry.slug))
        .collect();

    let mut taken: Vec<Name> = entries
        .iter()
        .zip(orders)
        .zip(&keeps_slug)
        .filter(|(_, keeps)| **keeps)
        .map(|((entry, &order), _)| Name {
            order,
            slug: entry.slug.clone(),
        })
        .collect();

    entries
        .iter()
        .zip(orders)
        .zip(keeps_slug)
        .map(|((entry, &order), keeps)| {
            if keeps {
                return Name {
                    order,
                    slug: entry.slug.clone(),
                };
            }
            let name = Name {
                order,
                slug: unique_slug(&taken, entry.slug.clone()),
            };
            taken.push(name.clone());
            name
        })
        .collect()
}

/// Renames for every entry whose name differs from `names`, ordered to avoid clashes.
fn renames_to(entries: &[Entry], dir: &Path, names: &[Name]) -> Vec<Op> {
    let renames = entries
        .iter()
        .zip(names)
        .map(|(entry, name)| {
            let to = dir.join(format!("{}{}", name.to_stem(), entry.suffix));
            (entry.path.clone(), to)
        })
        .filter(|(from, to)| from != to)
        .collect();
    order_renames(renames)
}

/// `o` / `O`: writes `task` into `section` so it ends up at position `index`.
///
/// The filename gets an order between its new neighbours and a slug from the title.
/// If there is no free order at that spot, the section is renumbered first (same op list,
/// so one undo reverts both). Returns `None` only if the section is full.
pub fn insert(section: &Section, index: usize, task: &Task) -> Option<Vec<Op>> {
    let entries = task_entries(section);
    let (mut ops, name) = place(&entries, &section.dir, index, slugify(&task.title()))?;
    ops.push(Op::Create {
        path: section.dir.join(format!("{}.md", name.to_stem())),
        content: task.content.clone(),
    });
    Some(ops)
}

/// `J` / `K` on a task: moves task `t` of section `from` to position `index` of section `to`.
///
/// `index` counts the tasks of `to` without the moved one. Like [`insert`], but the file is
/// renamed instead of created, so its content is never touched. `None` if `to` is full.
pub fn move_task(
    sections: &[Section],
    from: usize,
    t: usize,
    to: usize,
    index: usize,
) -> Option<Vec<Op>> {
    let task = &sections[from].tasks[t];
    let target = &sections[to];
    let others: Vec<Entry> = task_entries(target)
        .into_iter()
        .filter(|entry| entry.path != task.path)
        .collect();

    let (mut ops, name) = place(&others, &target.dir, index, wanted_slug(task))?;
    ops.push(Op::Rename {
        from: task.path.clone(),
        to: target.dir.join(format!("{}.md", name.to_stem())),
    });
    Some(ops)
}

/// `J` / `K` on a section header: moves section `s` one place down or up by renaming its
/// directory into the gap between its new neighbours.
///
/// `None` at the edge: the inbox never moves and nothing moves above it.
pub fn move_section(sections: &[Section], s: usize, down: bool) -> Option<Vec<Op>> {
    // Position among the section directories, the inbox is not one of them.
    let i = s.checked_sub(1)?;
    let entries = section_entries(sections);
    let index = if down { i + 1 } else { i.checked_sub(1)? };
    if index >= entries.len() {
        return None;
    }

    let mut others = entries;
    let moved = others.remove(i);
    let todo_dir = &sections[0].dir;
    let (mut ops, name) = place(&others, todo_dir, index, moved.slug.clone())?;
    ops.push(Op::Rename {
        from: moved.path.clone(),
        to: todo_dir.join(name.to_stem()),
    });
    Some(ops)
}

/// Name for a new entry at position `index` among `others` in `dir`: an order between its
/// new neighbours and a unique slug. Renumbers `others` if there is no gap (their renames are
/// returned). `None` if there is no room even after renumbering.
fn place(others: &[Entry], dir: &Path, index: usize, slug: String) -> Option<(Vec<Op>, Name)> {
    let (ops, names, order) = match free_order(others, index) {
        Some(order) => {
            let names = others
                .iter()
                .filter_map(|entry| entry.name.clone())
                .collect();
            (Vec::new(), names, order)
        }
        None => renumber(others, dir, index)?,
    };
    let name = Name {
        order,
        slug: unique_slug(&names, slug),
    };
    Some((ops, name))
}

/// An unused order between the entries around `index`.
///
/// `None` if there is no gap, or if a neighbour has no prefix (then there is nothing
/// to place the new entry relative to).
fn free_order(entries: &[Entry], index: usize) -> Option<u32> {
    let order_of = |entry: &Entry| entry.name.as_ref().map(|name| name.order);

    // The `?` inside these returns `None` from `free_order` if a neighbour has no prefix.
    let before = match index.checked_sub(1) {
        Some(i) => Some(order_of(&entries[i])?),
        None => None,
    };
    let after = match entries.get(index) {
        Some(entry) => Some(order_of(entry)?),
        None => None,
    };
    order_between(before, after)
}

/// Renumbers all entries with even gaps, leaving position `slot` free for a new one.
///
/// Returns the renames, the new names of all entries and the order for the free slot.
fn renumber(entries: &[Entry], dir: &Path, slot: usize) -> Option<(Vec<Op>, Vec<Name>, u32)> {
    let gap = gap_for(entries.len() + 1)?;
    let order_at = |position: usize| (position as u32 + 1) * gap;
    let orders: Vec<u32> = (0..entries.len())
        .map(|i| order_at(if i < slot { i } else { i + 1 }))
        .collect();

    let names = target_names(entries, &orders);
    Some((renames_to(entries, dir, &names), names, order_at(slot)))
}

/// The largest of 10, 5, 2, 1 that fits `count` entries into 999.
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

    /// Path of the file an insert creates, the last op.
    fn inserted_path(ops: Option<Vec<Op>>) -> Option<PathBuf> {
        match ops?.last() {
            Some(Op::Create { path, .. }) => Some(path.clone()),
            other => panic!("expected a create at the end, got {other:?}"),
        }
    }

    #[test]
    fn delete_removes_with_content() {
        let t = task("Fix bug");
        assert_eq!(
            delete(&t),
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
    fn insert_keeps_content() {
        let ops = insert(&section(&[]), 0, &task("Fix bug")).unwrap();
        let [Op::Create { content, .. }] = ops.as_slice() else {
            panic!("expected a single create");
        };
        assert_eq!(content, "# Fix bug\n\nbody");
    }

    #[test]
    fn insert_positions() {
        let s = section(&["010-a.md", "020-b.md"]);
        let path = |index| inserted_path(insert(&s, index, &task("New")));

        assert_eq!(path(0), Some(Path::new("todo/work/005-new.md").into()));
        assert_eq!(path(1), Some(Path::new("todo/work/015-new.md").into()));
        assert_eq!(path(2), Some(Path::new("todo/work/030-new.md").into()));
    }

    #[test]
    fn insert_into_empty_section() {
        let path = inserted_path(insert(&section(&[]), 0, &task("New")));
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
    fn insert_without_gap_renumbers() {
        let s = section(&["010-a.md", "011-b.md", "012-c.md"]);
        assert_eq!(
            describe(insert(&s, 1, &task("New"))).unwrap(),
            [
                "rename 011-b.md -> 030-b.md",
                "rename 012-c.md -> 040-c.md",
                "create 020-new.md",
            ]
        );
    }

    #[test]
    fn insert_next_to_unprefixed_renumbers() {
        let s = section(&["010-a.md", "My Notes.md"]);
        assert_eq!(
            describe(insert(&s, 2, &task("New"))).unwrap(),
            ["rename My Notes.md -> 020-My Notes.md", "create 030-new.md"]
        );
        // Before the first task needs no renumber, its neighbour has a prefix.
        assert_eq!(
            describe(insert(&s, 0, &task("New"))).unwrap(),
            ["create 005-new.md"]
        );
    }

    #[test]
    fn renames_are_ordered_to_avoid_clashes() {
        // `009-x` becomes `010-x`, which is free only after the old `010-x` moved away.
        let s = section(&["009-x.md", "010-x.md"]);
        assert_eq!(
            describe(insert(&s, 1, &task("New"))).unwrap(),
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
        let ops = describe(insert(&s, 1, &task("New"))).unwrap();
        assert!(ops.contains(&"create 010-new.md".to_string()));
        assert!(ops.contains(&"rename 100-t100.md -> 505-t100.md".to_string()));
    }

    #[test]
    fn full_section_cannot_insert() {
        let files: Vec<String> = (1..=999).map(|i| format!("{i:03}-t{i}.md")).collect();
        let s = section(&files.iter().map(String::as_str).collect::<Vec<_>>());
        assert_eq!(insert(&s, 1, &task("New")), None);
    }

    #[test]
    fn insert_slug_collision() {
        let s = section(&["010-new.md", "020-new-2.md"]);
        let path = inserted_path(insert(&s, 2, &task("New")));
        assert_eq!(path, Some(Path::new("todo/work/030-new-3.md").into()));
    }

    /// Inbox in `todo/` plus section dirs, each with task files (no headings).
    fn tree(spec: &[(&str, &[&str])]) -> Vec<Section> {
        spec.iter()
            .map(|(dir, files)| {
                let dir = Path::new("todo").join(dir);
                Section {
                    path: vec![],
                    tasks: files
                        .iter()
                        .map(|file| Task::parse(dir.join(file), ""))
                        .collect(),
                    dir,
                }
            })
            .collect()
    }

    #[test]
    fn move_task_within_section_renames_once() {
        let s = tree(&[("", &["010-a.md", "020-b.md", "030-c.md"])]);
        // `a` down: after `b`, between `b` and `c`.
        assert_eq!(
            describe(move_task(&s, 0, 0, 0, 1)).unwrap(),
            ["rename 010-a.md -> 025-a.md"]
        );
        // `c` up: before `b`, between `a` and `b`.
        assert_eq!(
            describe(move_task(&s, 0, 2, 0, 1)).unwrap(),
            ["rename 030-c.md -> 015-c.md"]
        );
    }

    #[test]
    fn move_task_into_other_section() {
        let s = tree(&[("", &["010-a.md"]), ("010-work", &["010-b.md"])]);
        let ops = move_task(&s, 0, 0, 1, 0).unwrap();
        assert_eq!(
            ops,
            [Op::Rename {
                from: "todo/010-a.md".into(),
                to: "todo/010-work/005-a.md".into(),
            }]
        );
    }

    #[test]
    fn move_task_keeps_slug_unique_in_target() {
        let s = tree(&[("", &["010-a.md"]), ("010-work", &["010-a.md"])]);
        assert_eq!(
            describe(move_task(&s, 0, 0, 1, 1)).unwrap(),
            ["rename 010-a.md -> 020-a-2.md"]
        );
    }

    #[test]
    fn move_task_renumbers_without_gap() {
        let s = tree(&[("", &["010-a.md", "011-b.md", "012-c.md"])]);
        // `c` up between `a` and `b`: no gap, so `b` moves away first.
        assert_eq!(
            describe(move_task(&s, 0, 2, 0, 1)).unwrap(),
            ["rename 011-b.md -> 030-b.md", "rename 012-c.md -> 020-c.md"]
        );
    }

    #[test]
    fn move_section_into_gap() {
        let s = tree(&[
            ("", &[]),
            ("010-work", &[]),
            ("020-home", &[]),
            ("030-x", &[]),
        ]);
        assert_eq!(
            describe(move_section(&s, 1, true)).unwrap(),
            ["rename 010-work -> 025-work"]
        );
        assert_eq!(
            describe(move_section(&s, 3, false)).unwrap(),
            ["rename 030-x -> 015-x"]
        );
    }

    #[test]
    fn move_section_stops_at_edges() {
        let s = tree(&[("", &[]), ("010-work", &[]), ("020-home", &[])]);
        assert_eq!(move_section(&s, 0, true), None, "inbox never moves");
        assert_eq!(
            move_section(&s, 1, false),
            None,
            "nothing moves above the inbox"
        );
        assert_eq!(move_section(&s, 2, true), None, "already last");
    }

    #[test]
    fn move_section_renumbers_and_prefixes_hand_made_dirs() {
        let s = tree(&[("", &[]), ("010-work", &[]), ("home", &[])]);
        // `home` up before `work`: room before `010`, one rename.
        assert_eq!(
            describe(move_section(&s, 2, false)).unwrap(),
            ["rename home -> 005-home"]
        );
        // `work` down after `home`: no order next to an unprefixed dir, so both are renumbered.
        assert_eq!(
            describe(move_section(&s, 1, true)).unwrap(),
            ["rename home -> 010-home", "rename 010-work -> 020-work"]
        );
    }
}
