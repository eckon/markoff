//! Turns user actions into [`Op`]s. Decides what should happen, applying is up to the caller.
//!
//! Every action writes the new order into the state file (see [`crate::state`]) along with its
//! file ops, so one undo reverts both. Files are only renamed when their title changes or they
//! move to another section.

use std::path::{Path, PathBuf};

use jiff::civil::Date;

use crate::{
    archive,
    name::slugify,
    op::Op,
    repeat,
    state::{Group, Item, State, StateFile},
    task::{EXTENSION, HEADING, Section, Task, split_heading},
};

/// `D`: removes task `t` of section `s`.
pub fn delete(sections: &[Section], s: usize, t: usize, file: &StateFile) -> Vec<Op> {
    let task = &sections[s].tasks[t];
    let mut state = State::of(sections);
    state.sections[s].tasks.remove(t);

    let mut ops = vec![Op::Remove {
        path: task.path.clone(),
        content: task.content.clone(),
    }];
    ops.extend(file.write(&state));
    ops
}

/// `D` on a section header: removes the directory of section `s`. Only for an empty section,
/// the caller checks.
pub fn delete_section(sections: &[Section], s: usize, file: &StateFile) -> Vec<Op> {
    let mut state = State::of(sections);
    state.sections.remove(s);

    let mut ops = vec![Op::RemoveDir {
        path: sections[s].dir.clone(),
    }];
    ops.extend(file.write(&state));
    ops
}

/// In the section draft: a section is a directory, there is no file for text to go into.
const SECTION_NOTE: &str = "<!-- Only the heading is used: it becomes the section name. -->";

/// What the editor shows to create (`N`, empty `title`) or rename (`<CR>`) a section.
pub fn section_draft(title: &str) -> String {
    format!("{HEADING}{title}\n\n{SECTION_NOTE}\n")
}

/// The section name from a saved [`section_draft`]: the heading as slug, and whether
/// there was more text than the heading and the note. `None` for an empty heading.
pub fn section_name(content: &str) -> Option<(String, bool)> {
    let (heading, body) = split_heading(content);
    let slug = slugify(&heading?);
    let more = !body.replace(SECTION_NOTE, "").trim().is_empty();
    Some((slug, more))
}

/// Renames section `s` to `slug`, keeping its place. Returns the ops and the new directory.
pub fn rename_section(
    sections: &[Section],
    s: usize,
    slug: &str,
    file: &StateFile,
) -> (Vec<Op>, PathBuf) {
    let from = sections[s].dir.clone();
    let to = from.with_file_name(slug);
    let mut state = State::of(sections);
    state.sections[s].name = slug.to_string();

    let mut ops = vec![Op::Rename {
        from,
        to: to.clone(),
    }];
    ops.extend(file.write(&state));
    (ops, to)
}

/// `X` on a repeating task: `date` becomes its last done in the state file. The task file
/// stays as it is and nothing is archived, see [`crate::repeat`].
pub fn repeat_done(
    sections: &[Section],
    s: usize,
    t: usize,
    date: Date,
    file: &StateFile,
) -> Vec<Op> {
    let mut state = State::of(sections);
    state.sections[s].tasks[t].done = Some(date);
    file.write(&state).into_iter().collect()
}

/// `X`: adds task `t` of section `s` to today's archive file, then removes it like [`delete`].
///
/// `existing` is the current content of `archive_path`, `None` if the file does not exist.
/// Archive first: if removing the task fails, the rollback takes the entry out again.
pub fn archive(
    sections: &[Section],
    s: usize,
    t: usize,
    archive_path: &Path,
    existing: Option<&str>,
    date: &str,
    file: &StateFile,
) -> Vec<Op> {
    let section = &sections[s];
    let after = archive::add(existing, date, &section.name, &section.tasks[t]);
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
    ops.extend(delete(sections, s, t, file));
    ops
}

/// A task file, as far as its name is concerned.
struct Entry {
    path: PathBuf,
    /// The name it has now. `None` if it comes from another section and has to make room for
    /// the names already there.
    current: Option<String>,
    /// The name it should have: from the heading, the current one without heading.
    slug: String,
    /// Goes along with the name, see [`Task::done`].
    done: Option<Date>,
}

impl Entry {
    fn task(task: &Task) -> Self {
        Self {
            path: task.path.clone(),
            current: Some(task.stem().to_string()),
            // Without heading the name stays (`My Notes.md`).
            slug: match &task.heading {
                Some(heading) => slugify(heading),
                None => task.stem().to_string(),
            },
            done: task.done,
        }
    }
}

/// The tasks of a section as [`Entry`]s, in display order.
fn task_entries(section: &Section) -> Vec<Entry> {
    section.tasks.iter().map(Entry::task).collect()
}

/// Renames tasks whose filename does not match their heading, and writes the state file so
/// it lists exactly what is on disk. Tasks without heading keep their name. Returns no ops
/// if everything already matches.
pub fn normalize(sections: &[Section], file: &StateFile) -> Vec<Op> {
    let mut state = State::of(sections);
    let mut ops = Vec::new();
    for (s, section) in sections.iter().enumerate() {
        let (renames, items) = renames(&task_entries(section), &section.dir);
        ops.extend(renames);
        state.sections[s].tasks = items;
    }
    ops.extend(file.write(&state));
    ops
}

/// `o` / `O`: writes a task with `title` and `content` into section `s` at position `index`.
/// Returns the ops and the new path.
pub fn insert(
    sections: &[Section],
    s: usize,
    index: usize,
    title: &str,
    content: &str,
    file: &StateFile,
) -> (Vec<Op>, PathBuf) {
    let section = &sections[s];
    let taken: Vec<String> = section
        .tasks
        .iter()
        .map(|task| task.stem().to_string())
        .collect();
    let name = unique_slug(&taken, slugify(repeat::split(title).0));
    let path = section.dir.join(&name).with_extension(EXTENSION);
    let mut state = State::of(sections);
    state.sections[s]
        .tasks
        .insert(index, Item { name, done: None });

    let mut ops = vec![Op::Create {
        path: path.clone(),
        content: content.to_string(),
    }];
    ops.extend(file.write(&state));
    (ops, path)
}

/// `N`: creates section `slug` in `root` at position `index`. Returns the ops and the new
/// directory.
pub fn insert_section(
    sections: &[Section],
    root: &Path,
    index: usize,
    slug: &str,
    file: &StateFile,
) -> (Vec<Op>, PathBuf) {
    let path = root.join(slug);
    let mut state = State::of(sections);
    state.sections.insert(
        index,
        Group {
            name: slug.to_string(),
            tasks: Vec::new(),
        },
    );

    let mut ops = vec![Op::CreateDir { path: path.clone() }];
    ops.extend(file.write(&state));
    (ops, path)
}

/// `J` / `K` on a task: moves task `t` of section `from` to position `index` of section `to`.
///
/// `index` counts the tasks of `to` without the moved one. Within a section only the order
/// changes. Into another section the file is renamed, so its content is never touched.
/// Returns the ops and the new path.
pub fn move_task(
    sections: &[Section],
    from: usize,
    t: usize,
    to: usize,
    index: usize,
    file: &StateFile,
) -> (Vec<Op>, PathBuf) {
    let task = &sections[from].tasks[t];
    let mut state = State::of(sections);
    let item = state.sections[from].tasks.remove(t);

    let mut ops = Vec::new();
    let path = if from == to {
        state.sections[to].tasks.insert(index, item);
        task.path.clone()
    } else {
        // Coming from another section: give way to the names already there.
        let mut moved = Entry::task(task);
        moved.current = None;
        let mut target = task_entries(&sections[to]);
        target.insert(index, moved);

        let dir = &sections[to].dir;
        let (renames, items) = renames(&target, dir);
        ops.extend(renames);
        let path = dir.join(&items[index].name).with_extension(EXTENSION);
        state.sections[to].tasks = items;
        path
    };
    ops.extend(file.write(&state));
    (ops, path)
}

/// `J` / `K` on a section header: moves section `s` one place down or up. Returns the ops and
/// its directory, which stays the same.
///
/// `None` at the first or last place.
pub fn move_section(
    sections: &[Section],
    s: usize,
    down: bool,
    file: &StateFile,
) -> Option<(Vec<Op>, PathBuf)> {
    let other = if down { s + 1 } else { s.checked_sub(1)? };
    if other >= sections.len() {
        return None;
    }
    let mut state = State::of(sections);
    state.sections.swap(s, other);
    let ops = file.write(&state).into_iter().collect();
    Some((ops, sections[s].dir.clone()))
}

/// The renames that give `entries` in `dir` their wanted names, and the order items of all
/// entries.
fn renames(entries: &[Entry], dir: &Path) -> (Vec<Op>, Vec<Item>) {
    let names = target_names(entries);
    let renames = entries
        .iter()
        .zip(&names)
        .filter_map(|(entry, name)| {
            let to = dir.join(name).with_extension(EXTENSION);
            (entry.path != to).then(|| (entry.path.clone(), to))
        })
        .collect();
    let items = (entries.iter().zip(names))
        .map(|(entry, name)| Item {
            name,
            done: entry.done,
        })
        .collect();
    (order_renames(renames), items)
}

/// Names for all entries, with their wanted slug.
///
/// Entries whose name stays the same keep it, the others get a unique one (`-2`, ...), so
/// an entry never takes the name another one already has.
fn target_names(entries: &[Entry]) -> Vec<String> {
    let keeps_name = |entry: &Entry| entry.current.as_ref() == Some(&entry.slug);
    let mut taken: Vec<String> = entries
        .iter()
        .filter(|entry| keeps_name(entry))
        .map(|entry| entry.slug.clone())
        .collect();

    entries
        .iter()
        .map(|entry| {
            if keeps_name(entry) {
                entry.slug.clone()
            } else {
                let name = unique_slug(&taken, entry.slug.clone());
                taken.push(name.clone());
                name
            }
        })
        .collect()
}

/// Orders renames so each target is free when its turn comes.
///
/// Two tasks swapping titles: `a -> b` has to wait until the old `b` has moved on. Should no
/// order exist, the rest is kept as is: [`Op::Rename`] refuses to overwrite, so that ends in
/// an error and a rollback, never in a lost file.
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
fn unique_slug(taken: &[String], slug: String) -> String {
    let is_taken = |candidate: &str| taken.iter().any(|other| other == candidate);
    if !is_taken(&slug) {
        return slug;
    }
    (2..)
        .map(|n| format!("{slug}-{n}"))
        .find(|candidate| !is_taken(candidate))
        .expect("infinite iterator always finds a free slug")
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::*;

    /// Section dirs in `root/`, each with task files from `(file, content)` pairs.
    fn tree_with(spec: &[(&str, &[(&str, &str)])]) -> Vec<Section> {
        spec.iter()
            .map(|(dir, files)| {
                let path = Path::new("root").join(dir);
                Section {
                    name: dir.to_string(),
                    tasks: files
                        .iter()
                        .map(|(file, content)| Task::parse(path.join(file), content))
                        .collect(),
                    dir: path,
                }
            })
            .collect()
    }

    /// Like [`tree_with`], tasks without content (no heading).
    fn tree(spec: &[(&str, &[&str])]) -> Vec<Section> {
        let spec: Vec<(&str, Vec<(&str, &str)>)> = spec
            .iter()
            .map(|(dir, files)| (*dir, files.iter().map(|file| (*file, "")).collect()))
            .collect();
        let spec: Vec<(&str, &[(&str, &str)])> = spec
            .iter()
            .map(|(dir, files)| (*dir, files.as_slice()))
            .collect();
        tree_with(&spec)
    }

    /// The state file holding exactly the order of `sections`, as after a normalize.
    fn file_for(sections: &[Section]) -> StateFile {
        StateFile {
            path: PathBuf::from("root/.markoff"),
            content: Some(State::of(sections).format()),
        }
    }

    /// Ops as short strings with file names only: `rename a.md -> b.md`. A state file
    /// write shows the new content on one line: `state # work a b | # home c`.
    fn describe(ops: &[Op]) -> Vec<String> {
        let file = |path: &Path| path.file_name().unwrap().to_str().unwrap().to_string();
        let state = |content: &str| {
            let content = content.trim_end().replace("\n\n", " | ").replace('\n', " ");
            format!("state {content}")
        };
        ops.iter()
            .map(|op| match op {
                Op::Create { path, content } if file(path) == ".markoff" => state(content),
                Op::Update { path, after, .. } if file(path) == ".markoff" => state(after),
                Op::Create { path, .. } => format!("create {}", file(path)),
                Op::Remove { path, .. } => format!("remove {}", file(path)),
                Op::Rename { from, to } => format!("rename {} -> {}", file(from), file(to)),
                Op::Update { path, .. } => format!("update {}", file(path)),
                Op::CreateDir { path } => format!("mkdir {}", file(path)),
                Op::RemoveDir { path } => format!("rmdir {}", file(path)),
            })
            .collect()
    }

    #[test]
    fn delete_removes_with_content() {
        let s = tree_with(&[("work", &[("fix-bug.md", "# Fix bug\n\nbody"), ("b.md", "")])]);
        let ops = delete(&s, 0, 0, &file_for(&s));
        assert_eq!(
            ops[0],
            Op::Remove {
                path: "root/work/fix-bug.md".into(),
                content: "# Fix bug\n\nbody".to_string()
            }
        );
        assert_eq!(describe(&ops[1..]), ["state # work b"]);
    }

    #[test]
    fn delete_section_drops_it_from_the_order() {
        let s = tree(&[("a", &[]), ("b", &[]), ("c", &[])]);
        assert_eq!(
            describe(&delete_section(&s, 1, &file_for(&s))),
            ["rmdir b", "state # a | # c"]
        );
    }

    #[test]
    fn section_draft_round_trip() {
        let draft = section_draft("work");
        assert_eq!(section_name(&draft), Some(("work".to_string(), false)));

        let renamed = draft.replace("# work", "# Work Stuff");
        assert_eq!(
            section_name(&renamed),
            Some(("work-stuff".to_string(), false))
        );
        let more = format!("{renamed}\nnotes");
        assert_eq!(section_name(&more), Some(("work-stuff".to_string(), true)));

        assert_eq!(section_name(&draft.replace("# work", "# ")), None);
        assert_eq!(section_name("no heading"), None);
    }

    #[test]
    fn insert_section_at_the_given_place() {
        let s = tree(&[("a", &[]), ("b", &[])]);
        let root = Path::new("root");
        let (ops, path) = insert_section(&s, root, 1, "new", &file_for(&s));
        assert_eq!(describe(&ops), ["mkdir new", "state # a | # new | # b"]);
        assert_eq!(path, Path::new("root/new"));

        let (ops, _) = insert_section(&s, root, 0, "new", &file_for(&s));
        assert_eq!(describe(&ops), ["mkdir new", "state # new | # a | # b"]);

        // The very first section creates the state file.
        let none = StateFile {
            path: PathBuf::from("root/.markoff"),
            content: None,
        };
        let (ops, _) = insert_section(&[], root, 0, "new", &none);
        assert_eq!(describe(&ops), ["mkdir new", "state # new"]);
    }

    #[test]
    fn rename_section_keeps_its_place() {
        let s = tree(&[("a", &[]), ("work", &["x.md"])]);
        let (ops, to) = rename_section(&s, 1, "home", &file_for(&s));
        assert_eq!(
            describe(&ops),
            ["rename work -> home", "state # a | # home x"]
        );
        assert_eq!(to, Path::new("root/home"));
    }

    #[test]
    fn archive_creates_or_updates_then_removes() {
        let s = tree(&[("work", &["fix-bug.md", "b.md"])]);
        let path = Path::new(".archive/2026-10-14.md");
        let file = file_for(&s);

        let ops = archive(&s, 0, 0, path, None, "2026-10-14", &file);
        assert_eq!(
            describe(&ops),
            [
                "create 2026-10-14.md",
                "remove fix-bug.md",
                "state # work b"
            ]
        );

        let ops = archive(&s, 0, 0, path, Some("# 2026-10-14\n"), "2026-10-14", &file);
        assert_eq!(describe(&ops)[0], "update 2026-10-14.md");
    }

    #[test]
    fn normalize_leaves_clean_tree_alone() {
        let s = tree_with(&[("work", &[("a.md", "# A"), ("notes.md", "no heading")])]);
        assert_eq!(normalize(&s, &file_for(&s)), []);
    }

    #[test]
    fn normalize_syncs_name_to_heading_and_order() {
        let s = tree_with(&[
            ("work", &[("01-a.md", "# A"), ("old.md", "# New title")]),
            (
                "home",
                &[("Call Bob.md", "no heading"), ("x.md", "# Water `* * mon`")],
            ),
        ]);
        assert_eq!(
            describe(&normalize(&s, &file_for(&s))),
            [
                "rename 01-a.md -> a.md",
                "rename old.md -> new-title.md",
                // The repeat rule stays out of the filename.
                "rename x.md -> water.md",
                "state # work a new-title | # home Call Bob water",
            ]
        );
    }

    #[test]
    fn normalize_writes_a_missing_or_stale_state_file() {
        let s = tree(&[("work", &["a.md"]), ("home", &[])]);
        let ops = |content: Option<&str>| {
            let file = StateFile {
                path: PathBuf::from("root/.markoff"),
                content: content.map(str::to_string),
            };
            describe(&normalize(&s, &file))
        };
        assert_eq!(ops(None), ["state # work a | # home"]);
        assert_eq!(
            ops(Some("x\n# work\ngone\na\n# old\n")),
            ["state # work a | # home"]
        );
    }

    #[test]
    fn normalize_never_takes_a_used_name() {
        // `x-2` keeps its `-2` because `x` already has `x`.
        let s = tree_with(&[(
            "work",
            &[("x-2.md", "# X"), ("x.md", "# X"), ("old.md", "# X")],
        )]);
        assert_eq!(
            describe(&normalize(&s, &file_for(&s))),
            ["rename old.md -> x-3.md", "state # work x-2 x x-3"]
        );
    }

    #[test]
    fn renames_are_ordered_to_avoid_clashes() {
        // `b` is renamed to `c`, so `a` can take `b` only after that.
        let s = tree_with(&[("work", &[("a.md", "# B"), ("b.md", "# C")])]);
        assert_eq!(
            describe(&normalize(&s, &file_for(&s))),
            [
                "rename b.md -> c.md",
                "rename a.md -> b.md",
                "state # work b c"
            ]
        );
    }

    #[test]
    fn insert_keeps_content() {
        let s = tree(&[("work", &[])]);
        let (ops, path) = insert(&s, 0, 0, "Fix bug", "# Fix bug\n\nbody", &file_for(&s));
        let Op::Create { content, .. } = &ops[0] else {
            panic!("expected a create first");
        };
        assert_eq!(content, "# Fix bug\n\nbody");
        assert_eq!(path, Path::new("root/work/fix-bug.md"));
    }

    #[test]
    fn insert_names_a_repeating_task_without_its_rule() {
        let s = tree(&[("work", &[])]);
        let title = "Water plants `* * mon`";
        let (_, path) = insert(&s, 0, 0, title, &format!("# {title}"), &file_for(&s));
        assert_eq!(path, Path::new("root/work/water-plants.md"));
    }

    #[test]
    fn insert_only_writes_the_order() {
        let s = tree(&[("work", &["a.md", "b.md"])]);
        let insert_at = |index| describe(&insert(&s, 0, index, "New", "# New", &file_for(&s)).0);

        assert_eq!(insert_at(0), ["create new.md", "state # work new a b"]);
        assert_eq!(insert_at(1), ["create new.md", "state # work a new b"]);
        assert_eq!(insert_at(2), ["create new.md", "state # work a b new"]);
    }

    #[test]
    fn insert_name_collision() {
        let s = tree(&[("work", &["new.md", "new-2.md"])]);
        let (ops, path) = insert(&s, 0, 2, "New", "# New", &file_for(&s));
        assert_eq!(
            describe(&ops),
            ["create new-3.md", "state # work new new-2 new-3"]
        );
        assert_eq!(path, Path::new("root/work/new-3.md"));
    }

    #[test]
    fn move_task_within_section_only_writes_the_order() {
        let s = tree(&[("work", &["a.md", "b.md", "c.md"])]);

        // `a` down, after `b`.
        let (ops, path) = move_task(&s, 0, 0, 0, 1, &file_for(&s));
        assert_eq!(describe(&ops), ["state # work b a c"]);
        assert_eq!(path, Path::new("root/work/a.md"));

        // `c` up, before `b`.
        let (ops, _) = move_task(&s, 0, 2, 0, 1, &file_for(&s));
        assert_eq!(describe(&ops), ["state # work a c b"]);
    }

    #[test]
    fn move_task_into_other_section_renames_it() {
        let s = tree(&[("work", &["a.md", "b.md"]), ("home", &["c.md"])]);
        let (ops, path) = move_task(&s, 0, 0, 1, 0, &file_for(&s));
        assert_eq!(
            ops[0],
            Op::Rename {
                from: "root/work/a.md".into(),
                to: "root/home/a.md".into()
            }
        );
        assert_eq!(describe(&ops[1..]), ["state # work b | # home a c"]);
        assert_eq!(path, Path::new("root/home/a.md"));
    }

    #[test]
    fn move_task_keeps_name_unique_in_target() {
        let s = tree(&[("work", &["a.md"]), ("home", &["a.md"])]);
        let (ops, _) = move_task(&s, 0, 0, 1, 1, &file_for(&s));
        assert_eq!(
            describe(&ops),
            ["rename a.md -> a-2.md", "state # work | # home a a-2"]
        );
    }

    #[test]
    fn move_section_only_writes_the_order() {
        let s = tree(&[("work", &[]), ("home", &[]), ("x", &[])]);

        let (ops, path) = move_section(&s, 0, true, &file_for(&s)).unwrap();
        assert_eq!(describe(&ops), ["state # home | # work | # x"]);
        assert_eq!(path, Path::new("root/work"));

        let (ops, _) = move_section(&s, 2, false, &file_for(&s)).unwrap();
        assert_eq!(describe(&ops), ["state # work | # x | # home"]);
    }

    #[test]
    fn move_section_stops_at_edges() {
        let s = tree(&[("work", &[]), ("home", &[])]);
        let file = file_for(&s);
        assert_eq!(move_section(&s, 0, false, &file), None, "already first");
        assert_eq!(move_section(&s, 1, true, &file), None, "already last");
    }
}
