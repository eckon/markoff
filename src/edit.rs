//! Turns user actions into [`Op`]s. Decides what should happen, applying is up to the caller.
//!
//! Every action writes the new order into the state file (see [`crate::state`]) along with its
//! file ops, so one undo reverts both. Files are only renamed when their title changes or they
//! move to another section.

use std::path::{Path, PathBuf};

use jiff::civil::Date;

use crate::{
    archive::ArchiveDir,
    name::slugify,
    op::Op,
    repeat,
    state::{self, State, StateFile},
    task::{self, EXTENSION, HEADING, Section, Task, split_heading},
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

/// What the editor shows to create (`N`, empty `name`) or rename (`<CR>`) a section.
pub fn section_draft(name: &str) -> String {
    format!("{HEADING}{name}\n\n{SECTION_NOTE}\n")
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

/// `x` on a repeating task, `X` too: `date` becomes its last done in the state file. The task file
/// stays as it is and nothing is archived, see [`crate::repeat`].
pub fn repeat_done(
    sections: &[Section],
    s: usize,
    t: usize,
    date: Date,
    file: &StateFile,
) -> Vec<Op> {
    let mut state = State::of(sections);
    state.sections[s].tasks[t].last_done = Some(date);
    file.write(&state).into_iter().collect()
}

/// `x`, and `X` with `cancel`: moves task `t` of section `s` into `to`, its section in today's
/// archive. As it is, a canceled one with the heading struck through first (see
/// [`task::cancel`]), unless it is already. Returns the ops and the new path.
pub fn archive(
    sections: &[Section],
    s: usize,
    t: usize,
    cancel: bool,
    to: &ArchiveDir,
    file: &StateFile,
) -> (Vec<Op>, PathBuf) {
    let task = &sections[s].tasks[t];
    let path = (to
        .path
        .join(unique_slug(&to.taken, task.stem().to_string())))
    .with_extension(EXTENSION);
    let mut state = State::of(sections);
    state.sections[s].tasks.remove(t);

    let mut ops: Vec<Op> = (to.missing.iter())
        .map(|path| Op::CreateDir { path: path.clone() })
        .collect();
    if cancel && !task.canceled {
        ops.push(Op::Update {
            path: task.path.clone(),
            before: task.content.clone(),
            after: task::cancel(&task.content, &task.title()),
        });
    }
    ops.push(Op::Rename {
        from: task.path.clone(),
        to: path.clone(),
    });
    ops.extend(file.write(&state));
    (ops, path)
}

/// A task file, as far as its name is concerned.
struct TaskName {
    path: PathBuf,
    /// The name it has now. `None` if it comes from another section and has to make room for
    /// the names already there.
    current: Option<String>,
    /// The name it should have: from the heading, the current one without heading.
    slug: String,
    /// Goes along with the name, see [`Task::last_done`].
    last_done: Option<Date>,
}

impl TaskName {
    fn task(task: &Task) -> Self {
        Self {
            path: task.path.clone(),
            current: Some(task.stem().to_string()),
            // Without heading the name stays (`My Notes.md`).
            slug: match &task.heading {
                Some(heading) => slugify(heading),
                None => task.stem().to_string(),
            },
            last_done: task.last_done,
        }
    }
}

/// The tasks of a section as [`TaskName`]s, in display order.
fn task_names(section: &Section) -> Vec<TaskName> {
    section.tasks.iter().map(TaskName::task).collect()
}

/// Renames tasks whose filename does not match their heading, and writes the state file so
/// it lists exactly what is on disk. Tasks without heading keep their name. Returns no ops
/// if everything already matches.
pub fn normalize(sections: &[Section], file: &StateFile) -> Vec<Op> {
    let mut state = State::of(sections);
    let mut ops = Vec::new();
    for (s, section) in sections.iter().enumerate() {
        let (renames, lines) = renames(&task_names(section), &section.dir);
        ops.extend(renames);
        state.sections[s].tasks = lines;
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
    state.sections[s].tasks.insert(
        index,
        state::Task {
            name,
            last_done: None,
        },
    );

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
        state::Section {
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
    let line = state.sections[from].tasks.remove(t);

    let mut ops = Vec::new();
    let path = if from == to {
        state.sections[to].tasks.insert(index, line);
        task.path.clone()
    } else {
        // Coming from another section: give way to the names already there.
        let mut moved = TaskName::task(task);
        moved.current = None;
        let mut target = task_names(&sections[to]);
        target.insert(index, moved);

        let dir = &sections[to].dir;
        let (renames, lines) = renames(&target, dir);
        ops.extend(renames);
        let path = dir.join(&lines[index].name).with_extension(EXTENSION);
        state.sections[to].tasks = lines;
        path
    };
    ops.extend(file.write(&state));
    (ops, path)
}

/// `J` / `K` on a section header: moves section `s` to position `index`, counted without it.
/// Only the order changes. Returns the ops and its directory, which stays the same.
pub fn move_section(
    sections: &[Section],
    s: usize,
    index: usize,
    file: &StateFile,
) -> (Vec<Op>, PathBuf) {
    let mut state = State::of(sections);
    let section = state.sections.remove(s);
    state.sections.insert(index, section);
    let ops = file.write(&state).into_iter().collect();
    (ops, sections[s].dir.clone())
}

/// The renames that give `tasks` in `dir` their wanted names, and their lines in the state
/// file, in the same order.
fn renames(tasks: &[TaskName], dir: &Path) -> (Vec<Op>, Vec<state::Task>) {
    let names = target_names(tasks);
    let renames = tasks
        .iter()
        .zip(&names)
        .filter_map(|(task, name)| {
            let to = dir.join(name).with_extension(EXTENSION);
            (task.path != to).then(|| (task.path.clone(), to))
        })
        .collect();
    let lines = (tasks.iter().zip(names))
        .map(|(task, name)| state::Task {
            name,
            last_done: task.last_done,
        })
        .collect();
    (order_renames(renames), lines)
}

/// Names for all tasks, with their wanted slug.
///
/// Tasks whose name stays the same keep it, the others get a unique one (`-2`, ...), so
/// a task never takes the name another one already has.
fn target_names(tasks: &[TaskName]) -> Vec<String> {
    let keeps_name = |task: &TaskName| task.current.as_ref() == Some(&task.slug);
    let mut taken: Vec<String> = tasks
        .iter()
        .filter(|task| keeps_name(task))
        .map(|task| task.slug.clone())
        .collect();

    tasks
        .iter()
        .map(|task| {
            if keeps_name(task) {
                task.slug.clone()
            } else {
                let name = unique_slug(&taken, task.slug.clone());
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
    fn archive_moves_into_missing_dirs() {
        let s = tree(&[("work", &["fix-bug.md", "b.md"])]);
        let dir = Path::new(".archive/2026-10-14/work");
        let file = file_for(&s);
        let to = |missing: Vec<PathBuf>, taken: &[&str]| ArchiveDir {
            path: dir.to_path_buf(),
            missing,
            taken: taken.iter().map(|name| name.to_string()).collect(),
        };

        let missing = vec![dir.parent().unwrap().to_path_buf(), dir.to_path_buf()];
        let (ops, path) = archive(&s, 0, 0, false, &to(missing, &[]), &file);
        assert_eq!(
            describe(&ops),
            [
                "mkdir 2026-10-14",
                "mkdir work",
                "rename fix-bug.md -> fix-bug.md",
                "state # work b"
            ]
        );
        assert_eq!(path, dir.join("fix-bug.md"));

        let (ops, path) = archive(&s, 0, 0, false, &to(vec![], &["fix-bug"]), &file);
        assert_eq!(describe(&ops)[0], "rename fix-bug.md -> fix-bug-2.md");
        assert_eq!(path, dir.join("fix-bug-2.md"));
    }

    #[test]
    fn cancel_strikes_the_heading() {
        let s = tree_with(&[("work", &[("a.md", "# A\n\nbody"), ("b.md", "# ~~B~~")])]);
        let to = ArchiveDir {
            path: PathBuf::from(".archive/2026-10-14/work"),
            missing: vec![],
            taken: vec![],
        };
        let (ops, _) = archive(&s, 0, 0, true, &to, &file_for(&s));
        assert_eq!(describe(&ops)[..2], ["update a.md", "rename a.md -> a.md"]);
        let Op::Update { after, .. } = &ops[0] else {
            unreachable!()
        };
        assert_eq!(after, "# ~~A~~\n\nbody");
        let (ops, _) = archive(&s, 0, 1, true, &to, &file_for(&s));
        assert_eq!(
            describe(&ops)[0],
            "rename b.md -> b.md",
            "struck through already"
        );
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

        let (ops, path) = move_section(&s, 0, 1, &file_for(&s));
        assert_eq!(describe(&ops), ["state # home | # work | # x"]);
        assert_eq!(path, Path::new("root/work"));

        let (ops, _) = move_section(&s, 2, 0, &file_for(&s));
        assert_eq!(describe(&ops), ["state # x | # work | # home"]);
        assert_eq!(move_section(&s, 1, 1, &file_for(&s)).0, [], "same place");
    }
}
