//! Turns user actions into [`Op`]s. Decides what should happen, applying is up to the caller.
//!
//! Every action leaves the order clean: `010`, `020`, `030`, ... for the tasks of a section
//! and for the section directories. An action computes the new order and [`renumber`]s it.

use std::path::{Path, PathBuf};

use crate::{
    archive,
    name::{MAX_ORDER, Name, ORDER_GAP, slugify},
    op::Op,
    task::{Section, Task},
};

/// `D`: removes task `t` and renumbers the rest of its section.
pub fn delete(section: &Section, t: usize) -> Vec<Op> {
    let task = &section.tasks[t];
    let mut ops = vec![Op::Remove {
        path: task.path.clone(),
        content: task.content.clone(),
    }];
    let mut rest = task_entries(section);
    rest.remove(t);
    ops.extend(
        renumber(&rest, &section.dir)
            .map(|(renames, _)| renames)
            .unwrap_or_default(),
    );
    ops
}

/// `X`: adds task `t` to today's archive file, then removes it like [`delete`].
///
/// `existing` is the current content of `archive_path`, `None` if the file does not exist.
/// Archive first: if removing the task fails, the rollback takes the entry out again.
pub fn archive(
    section: &Section,
    t: usize,
    archive_path: &Path,
    existing: Option<&str>,
    date: &str,
) -> Vec<Op> {
    let after = archive::add(existing, date, &section.title(), &section.tasks[t]);
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
    ops.extend(delete(section, t));
    ops
}

/// A task file or section directory, as far as naming and order are concerned.
#[derive(Debug, Clone)]
struct Entry {
    /// `None` for a task that does not exist yet.
    path: Option<PathBuf>,
    /// `None` without a valid `NNN-` prefix, or if the entry comes from somewhere else and
    /// has to make room for the ones already there.
    name: Option<Name>,
    /// The slug it should have: from the heading for tasks, the current one otherwise.
    slug: String,
    /// `.md` for task files, empty for directories.
    suffix: &'static str,
}

impl Entry {
    fn task(task: &Task) -> Self {
        Self {
            path: Some(task.path.clone()),
            name: task.name.clone(),
            slug: wanted_slug(task),
            suffix: ".md",
        }
    }
}

/// The tasks of a section as [`Entry`]s, in display order.
fn task_entries(section: &Section) -> Vec<Entry> {
    section.tasks.iter().map(Entry::task).collect()
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
                path: Some(section.dir.clone()),
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

/// Renames the tasks of a section into the shape markoff itself creates: clean orders in the
/// order they are shown (unprefixed ones last), slugs from the heading. Tasks without heading
/// keep their name. Returns no ops if everything already matches.
pub fn normalize(section: &Section) -> Vec<Op> {
    renumber(&task_entries(section), &section.dir)
        .map(|(renames, _)| renames)
        .unwrap_or_default()
}

/// Like [`normalize`] for the section directories in `todo/`. The inbox is `todo/` itself
/// and never renamed.
pub fn normalize_sections(sections: &[Section]) -> Vec<Op> {
    renumber(&section_entries(sections), &sections[0].dir)
        .map(|(renames, _)| renames)
        .unwrap_or_default()
}

/// `o` / `O`: writes `task` into `section` so it ends up at position `index`.
///
/// The section is renumbered with the new task in it (same op list, so one undo reverts
/// both). Returns `None` only if the section is full.
pub fn insert(section: &Section, index: usize, task: &Task) -> Option<Vec<Op>> {
    let mut entries = task_entries(section);
    entries.insert(
        index,
        Entry {
            path: None,
            name: None,
            slug: slugify(&task.title()),
            suffix: ".md",
        },
    );
    let (mut ops, names) = renumber(&entries, &section.dir)?;
    ops.push(Op::Create {
        path: section.dir.join(format!("{}.md", names[index].to_stem())),
        content: task.content.clone(),
    });
    Some(ops)
}

/// `J` / `K` on a task: moves task `t` of section `from` to position `index` of section `to`.
///
/// `index` counts the tasks of `to` without the moved one. Both sections are renumbered, the
/// file is only renamed, so its content is never touched. Returns the ops and the new path,
/// `None` if `to` is full.
pub fn move_task(
    sections: &[Section],
    from: usize,
    t: usize,
    to: usize,
    index: usize,
) -> Option<(Vec<Op>, PathBuf)> {
    let mut moved = Entry::task(&sections[from].tasks[t]);
    let mut ops = Vec::new();
    let mut target = task_entries(&sections[to]);
    if from == to {
        target.remove(t);
    } else {
        // Coming from another section: give way to the slugs already there.
        moved.name = None;
        let mut rest = task_entries(&sections[from]);
        rest.remove(t);
        ops.extend(renumber(&rest, &sections[from].dir)?.0);
    }
    target.insert(index, moved);

    let dir = &sections[to].dir;
    let (renames, names) = renumber(&target, dir)?;
    ops.extend(renames);
    Some((ops, dir.join(format!("{}.md", names[index].to_stem()))))
}

/// `J` / `K` on a section header: moves section `s` one place down or up, the section
/// directories are renumbered. Returns the ops and the new directory.
///
/// `None` at the edge: the inbox never moves and nothing moves above it.
pub fn move_section(sections: &[Section], s: usize, down: bool) -> Option<(Vec<Op>, PathBuf)> {
    // Position among the section directories, the inbox is not one of them.
    let i = s.checked_sub(1)?;
    let mut entries = section_entries(sections);
    let index = if down { i + 1 } else { i.checked_sub(1)? };
    if index >= entries.len() {
        return None;
    }
    let moved = entries.remove(i);
    entries.insert(index, moved);

    let todo_dir = &sections[0].dir;
    let (ops, names) = renumber(&entries, todo_dir)?;
    Some((ops, todo_dir.join(names[index].to_stem())))
}

/// Numbers `entries` in this order `010`, `020`, ... in `dir`, with their wanted slugs.
///
/// Returns the renames for entries whose path changes, and the names of all entries.
/// `None` if there are too many for 999.
fn renumber(entries: &[Entry], dir: &Path) -> Option<(Vec<Op>, Vec<Name>)> {
    let gap = gap_for(entries.len())?;
    let orders: Vec<u32> = (1..=entries.len() as u32)
        .map(|position| position * gap)
        .collect();
    let names = target_names(entries, &orders);

    let renames = entries
        .iter()
        .zip(&names)
        .filter_map(|(entry, name)| {
            let from = entry.path.clone()?;
            let to = dir.join(format!("{}{}", name.to_stem(), entry.suffix));
            (from != to).then_some((from, to))
        })
        .collect();
    Some((order_renames(renames), names))
}

/// The largest of 10, 5, 2, 1 that fits `count` entries into 999.
fn gap_for(count: usize) -> Option<u32> {
    [ORDER_GAP, 5, 2, 1]
        .into_iter()
        .find(|gap| count as u32 * gap <= MAX_ORDER)
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
        section_with(&files.iter().map(|file| (*file, "")).collect::<Vec<_>>())
    }

    /// A section in `todo/work` from `(file, content)` pairs.
    fn section_with(files: &[(&str, &str)]) -> Section {
        let dir = PathBuf::from("todo/work");
        Section {
            path: vec!["work".to_string()],
            tasks: files
                .iter()
                .map(|(file, content)| Task::parse(dir.join(file), content))
                .collect(),
            dir,
        }
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

    fn task(title: &str) -> Task {
        Task::parse(PathBuf::from("todo/old.md"), &format!("# {title}\n\nbody"))
    }

    /// Ops as short strings with file names only: `rename 011-b.md -> 030-b.md`.
    fn describe(ops: &[Op]) -> Vec<String> {
        let file = |path: &Path| path.file_name().unwrap().to_str().unwrap().to_string();
        ops.iter()
            .map(|op| match op {
                Op::Create { path, .. } => format!("create {}", file(path)),
                Op::Remove { path, .. } => format!("remove {}", file(path)),
                Op::Rename { from, to } => format!("rename {} -> {}", file(from), file(to)),
                Op::Update { path, .. } => format!("update {}", file(path)),
            })
            .collect()
    }

    #[test]
    fn delete_removes_with_content() {
        let s = section_with(&[("010-fix-bug.md", "# Fix bug\n\nbody")]);
        assert_eq!(
            delete(&s, 0),
            [Op::Remove {
                path: "todo/work/010-fix-bug.md".into(),
                content: "# Fix bug\n\nbody".to_string()
            }]
        );
    }

    #[test]
    fn delete_closes_the_gap() {
        let s = section(&["010-a.md", "020-b.md", "030-c.md"]);
        assert_eq!(
            describe(&delete(&s, 0)),
            [
                "remove 010-a.md",
                "rename 020-b.md -> 010-b.md",
                "rename 030-c.md -> 020-c.md",
            ]
        );
    }

    #[test]
    fn archive_creates_or_updates_then_removes() {
        let s = section(&["010-fix-bug.md", "020-b.md"]);
        let path = Path::new("archive/2026-10-14.md");

        let ops = archive(&s, 0, path, None, "2026-10-14");
        assert_eq!(
            describe(&ops),
            [
                "create 2026-10-14.md",
                "remove 010-fix-bug.md",
                "rename 020-b.md -> 010-b.md",
            ]
        );

        let ops = archive(&s, 0, path, Some("# 2026-10-14\n"), "2026-10-14");
        assert_eq!(describe(&ops)[0], "update 2026-10-14.md");
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
            describe(&normalize(&s)),
            ["rename 010-old.md -> 010-new-title.md"]
        );
    }

    #[test]
    fn normalize_cleans_orders_and_adds_prefixes() {
        // Loaded order: prefixed first, then unprefixed by name.
        let s = section_with(&[
            ("005-a.md", "# A"),
            ("017-b.md", "# B"),
            ("Call Bob.md", "no heading"),
            ("random.md", "# Fix login"),
        ]);
        assert_eq!(
            describe(&normalize(&s)),
            [
                "rename 005-a.md -> 010-a.md",
                "rename 017-b.md -> 020-b.md",
                "rename Call Bob.md -> 030-Call Bob.md",
                "rename random.md -> 040-fix-login.md",
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
            describe(&normalize(&s)),
            ["rename 030-old.md -> 030-x-3.md"]
        );
    }

    #[test]
    fn renames_are_ordered_to_avoid_clashes() {
        // `009-x` becomes `010-x`, which is free only after the old `010-x` moved away.
        let s = section(&["009-x.md", "010-x.md"]);
        assert_eq!(
            describe(&normalize(&s)),
            ["rename 010-x.md -> 020-x.md", "rename 009-x.md -> 010-x.md"]
        );
    }

    #[test]
    fn normalize_sections_cleans_dirs() {
        let s = tree(&[
            ("", &[]),
            ("005-markoff", &[]),
            ("017-private", &[]),
            ("home", &[]),
        ]);
        assert_eq!(
            describe(&normalize_sections(&s)),
            [
                "rename 005-markoff -> 010-markoff",
                "rename 017-private -> 020-private",
                "rename home -> 030-home",
            ]
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
    fn insert_renumbers_the_section() {
        let s = section(&["010-a.md", "020-b.md"]);
        let insert_at = |index| describe(&insert(&s, index, &task("New")).unwrap());

        assert_eq!(
            insert_at(0),
            [
                "rename 010-a.md -> 020-a.md",
                "rename 020-b.md -> 030-b.md",
                "create 010-new.md",
            ]
        );
        assert_eq!(
            insert_at(1),
            ["rename 020-b.md -> 030-b.md", "create 020-new.md"]
        );
        assert_eq!(insert_at(2), ["create 030-new.md"]);
    }

    #[test]
    fn insert_gives_unprefixed_a_prefix() {
        let s = section(&["010-a.md", "My Notes.md"]);
        assert_eq!(
            describe(&insert(&s, 2, &task("New")).unwrap()),
            ["rename My Notes.md -> 020-My Notes.md", "create 030-new.md"]
        );
    }

    #[test]
    fn renumber_shrinks_gap_for_big_sections() {
        let files: Vec<String> = (1..=100).map(|i| format!("{i:03}-t{i}.md")).collect();
        let s = section(&files.iter().map(String::as_str).collect::<Vec<_>>());

        // 101 tasks with the new one: gap 10 would need 1010, gap 5 fits (505).
        let ops = describe(&insert(&s, 1, &task("New")).unwrap());
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
        let ops = insert(&s, 2, &task("New")).unwrap();
        assert_eq!(describe(&ops), ["create 030-new-3.md"]);
    }

    #[test]
    fn move_task_within_section() {
        let s = tree(&[("", &["010-a.md", "020-b.md", "030-c.md"])]);

        // `a` down, after `b`.
        let (ops, path) = move_task(&s, 0, 0, 0, 1).unwrap();
        assert_eq!(
            describe(&ops),
            ["rename 020-b.md -> 010-b.md", "rename 010-a.md -> 020-a.md"]
        );
        assert_eq!(path, Path::new("todo/020-a.md"));

        // `c` up, before `b`.
        let (ops, _) = move_task(&s, 0, 2, 0, 1).unwrap();
        assert_eq!(
            describe(&ops),
            ["rename 030-c.md -> 020-c.md", "rename 020-b.md -> 030-b.md"]
        );
    }

    #[test]
    fn move_task_into_other_section_renumbers_both() {
        let s = tree(&[("", &["010-a.md", "020-b.md"]), ("010-work", &["010-c.md"])]);
        let (ops, path) = move_task(&s, 0, 0, 1, 0).unwrap();
        let rename = |from: &str, to: &str| Op::Rename {
            from: from.into(),
            to: to.into(),
        };
        assert_eq!(
            ops,
            [
                rename("todo/020-b.md", "todo/010-b.md"),
                rename("todo/010-a.md", "todo/010-work/010-a.md"),
                rename("todo/010-work/010-c.md", "todo/010-work/020-c.md"),
            ]
        );
        assert_eq!(path, Path::new("todo/010-work/010-a.md"));
    }

    #[test]
    fn move_task_keeps_slug_unique_in_target() {
        let s = tree(&[("", &["010-a.md"]), ("010-work", &["010-a.md"])]);
        let (ops, _) = move_task(&s, 0, 0, 1, 1).unwrap();
        assert_eq!(describe(&ops), ["rename 010-a.md -> 020-a-2.md"]);
    }

    #[test]
    fn move_section_renumbers_dirs() {
        let s = tree(&[
            ("", &[]),
            ("010-work", &[]),
            ("020-home", &[]),
            ("030-x", &[]),
        ]);

        let (ops, path) = move_section(&s, 1, true).unwrap();
        assert_eq!(
            describe(&ops),
            ["rename 020-home -> 010-home", "rename 010-work -> 020-work"]
        );
        assert_eq!(path, Path::new("todo/020-work"));

        let (ops, _) = move_section(&s, 3, false).unwrap();
        assert_eq!(
            describe(&ops),
            ["rename 030-x -> 020-x", "rename 020-home -> 030-home"]
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
    fn move_section_prefixes_hand_made_dirs() {
        let s = tree(&[("", &[]), ("010-work", &[]), ("home", &[])]);
        let (ops, _) = move_section(&s, 2, false).unwrap();
        assert_eq!(
            describe(&ops),
            ["rename home -> 010-home", "rename 010-work -> 020-work"]
        );
    }
}
