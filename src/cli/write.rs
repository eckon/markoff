//! Commands that change files, the quick actions of the TUI and the editing it leaves to the
//! editor. Each finds its ids first, so a wrong one changes nothing, then normalizes like every
//! write in the TUI. Normalize keeps the order of sections and tasks, so what was found stays
//! valid, even a task it renames to match its title. Each prints the ids it ends with.

use std::path::{Path, PathBuf};

use clap::{ArgGroup, Args};
use color_eyre::{Result, eyre::bail};
use jiff::civil::Date;

use super::id::{self, Found};
use crate::{
    edit, load,
    name::slugify,
    op::{self, Op},
    state::StateFile,
    task::{HEADING, Section, split_heading},
};

#[derive(Debug, Clone, Default, PartialEq, Eq, Args)]
pub struct AddArgs {
    /// Section of the new task
    #[arg(
        required_unless_present = "new_section",
        conflicts_with = "new_section"
    )]
    section: Option<String>,
    /// Title of the new task, with its rule for a repeating one: "Water plants `* * mon`"
    #[arg(required_unless_present = "new_section")]
    title: Option<String>,
    /// Add a section named NAME instead of a task
    #[arg(long = "section", value_name = "NAME")]
    new_section: Option<String>,
    /// Body of the task, below the title, `-` reads it from stdin
    #[arg(
        long,
        value_name = "TEXT",
        conflicts_with = "new_section",
        allow_hyphen_values = true
    )]
    pub body: Option<String>,
    /// Put it before this task or section, instead of at the end
    #[arg(long, value_name = "ID", conflicts_with = "after")]
    before: Option<String>,
    /// Put it after this task or section, instead of at the end
    #[arg(long, value_name = "ID")]
    after: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Args)]
#[command(group(ArgGroup::new("change").required(true).multiple(true)))]
pub struct EditArgs {
    /// Id of the task or section
    id: String,
    /// New title of the task, with its rule for a repeating one, as `list` shows it
    #[arg(long, group = "change")]
    title: Option<String>,
    /// New body of the task, replacing the old one, `-` reads it from stdin
    #[arg(
        long,
        value_name = "TEXT",
        group = "change",
        conflicts_with = "append",
        allow_hyphen_values = true
    )]
    pub body: Option<String>,
    /// Text to add at the end of the task's body, `-` reads it from stdin
    #[arg(
        long,
        value_name = "TEXT",
        group = "change",
        allow_hyphen_values = true
    )]
    pub append: Option<String>,
    /// New name of the section
    #[arg(long, group = "change")]
    name: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Args)]
#[command(group(ArgGroup::new("place").required(true).multiple(true)))]
pub struct MoveArgs {
    /// Id of the task or section
    id: String,
    /// Section to move the task into, at the end unless another option says where
    #[arg(long, value_name = "SECTION", group = "place")]
    to: Option<String>,
    /// Put it before this task or section
    #[arg(long, value_name = "ID", group = "place", conflicts_with_all = ["after", "top", "bottom"])]
    before: Option<String>,
    /// Put it after this task or section
    #[arg(long, value_name = "ID", group = "place", conflicts_with_all = ["top", "bottom"])]
    after: Option<String>,
    /// Put it first, in its section for a task
    #[arg(long, group = "place", conflicts_with = "bottom")]
    top: bool,
    /// Put it last, in its section for a task
    #[arg(long, group = "place")]
    bottom: bool,
}

/// `o` at the end of a section, or before or after a task in it, with the body below the
/// heading. With `--section` a new section instead, like `N`, at the end or before or after
/// another one.
pub fn add(root: &Path, args: &AddArgs) -> Result<String> {
    let (sections, _) = load::load(root)?;
    let archive = load::archive(root)?;
    let find = |id: &str| id::find(id, root, &sections, &archive);

    if let Some(name) = &args.new_section {
        if name.trim().is_empty() {
            bail!("cannot add: empty name");
        }
        let slug = slugify(name);
        if sections.iter().any(|section| section.name == slug) {
            bail!("a section named '{slug}' already exists");
        }
        let index = match (&args.before, &args.after) {
            (Some(id), _) => section(id, find(id)?)?,
            (_, Some(id)) => section(id, find(id)?)? + 1,
            _ => sections.len(),
        };
        let (sections, file) = normalize(root)?;
        let (ops, _) = edit::insert_section(&sections, root, index, &slug, &file);
        op::apply_all(&ops)?;
        return Ok(format!("{slug}\n"));
    }

    let (Some(name), Some(title)) = (&args.section, &args.title) else {
        unreachable!("clap requires both without --section")
    };
    let title = title.trim();
    if title.is_empty() {
        bail!("cannot add: empty title");
    }
    let s = id::section(name, &id::names(&sections))?;
    let index = match (&args.before, &args.after) {
        (Some(id), _) => task_in(id, find(id)?, s, name)?,
        (_, Some(id)) => task_in(id, find(id)?, s, name)? + 1,
        _ => sections[s].tasks.len(),
    };
    let content = content(title, args.body.as_deref().unwrap_or_default());
    let (sections, file) = normalize(root)?;
    let (ops, path) = edit::insert(&sections, s, index, title, &content, &file);
    op::apply_all(&ops)?;
    Ok(format!("{}\n", id::of(root, &path)))
}

/// What `$EDITOR` does in the TUI: a new title or body for a task, or a new name for a
/// section, like `<CR>` on it.
pub fn edit(root: &Path, args: &EditArgs) -> Result<String> {
    let (sections, _) = load::load(root)?;
    let archive = load::archive(root)?;
    let id = &args.id;
    match id::find(id, root, &sections, &archive)? {
        Found::Archived(_) => bail!("'{id}' is archived, archived tasks are read-only"),
        Found::Section(s) => {
            let (Some(name), None, None, None) =
                (&args.name, &args.title, &args.body, &args.append)
            else {
                bail!("'{id}' is a section, only --name changes it");
            };
            if name.trim().is_empty() {
                bail!("cannot rename: empty name");
            }
            let slug = slugify(name);
            if slug == sections[s].name {
                return Ok(format!("{slug}\n"));
            }
            if sections.iter().any(|section| section.name == slug) {
                bail!("a section named '{slug}' already exists");
            }
            let (sections, file) = normalize(root)?;
            let (ops, _) = edit::rename_section(&sections, s, &slug, &file);
            op::apply_all(&ops)?;
            Ok(format!("{slug}\n"))
        }
        Found::Task(s, t) => {
            if args.name.is_some() {
                bail!("'{id}' is a task, --name is for sections, --title renames a task");
            }
            if args
                .title
                .as_deref()
                .is_some_and(|title| title.trim().is_empty())
            {
                bail!("cannot edit: empty title");
            }
            let (sections, _) = normalize(root)?;
            let task = &sections[s].tasks[t];
            let (heading, body) = split_heading(&task.content);
            let title = match &args.title {
                Some(title) => title.trim().to_string(),
                None => heading.unwrap_or_else(|| task.title()),
            };
            let body = match (&args.body, &args.append) {
                (Some(body), _) => body.clone(),
                (_, Some(text)) if body.trim().is_empty() => text.clone(),
                (_, Some(text)) => format!("{}\n{text}", body.trim_end()),
                _ => body.to_string(),
            };
            op::apply_all(&[Op::Update {
                path: task.path.clone(),
                before: task.content.clone(),
                after: content(&title, &body),
            }])?;
            // A new title renames the file, now rather than on the next write, so the id
            // printed is the one that stays.
            let (sections, _) = normalize(root)?;
            Ok(format!("{}\n", id::of(root, &sections[s].tasks[t].path)))
        }
    }
}

/// `J` / `K`, by more than one place: a task within its section or into another one, a
/// section among the others.
pub fn move_to(root: &Path, args: &MoveArgs) -> Result<String> {
    let (sections, _) = load::load(root)?;
    let archive = load::archive(root)?;
    let find = |id: &str| id::find(id, root, &sections, &archive);
    let id = &args.id;
    match find(id)? {
        Found::Archived(_) => bail!("'{id}' is archived, archived tasks are read-only"),
        Found::Section(s) => {
            if args.to.is_some() {
                bail!("'{id}' is a section, --to is for tasks, --before or --after moves it");
            }
            // Counted without the moved one.
            let place = |other: &str| match section(other, find(other)?)? {
                same if same == s => {
                    bail!("'{other}' is the section to move, it cannot go next to itself")
                }
                other => Ok(other - usize::from(s < other)),
            };
            let index = match (&args.before, &args.after) {
                (Some(other), _) => place(other)?,
                (_, Some(other)) => place(other)? + 1,
                _ if args.top => 0,
                _ => sections.len() - 1,
            };
            let (sections, file) = normalize(root)?;
            let (ops, _) = edit::move_section(&sections, s, index, &file);
            op::apply_all(&ops)?;
            Ok(format!("{}\n", sections[s].name))
        }
        Found::Task(s, t) => {
            let to = match &args.to {
                Some(name) => id::section(name, &id::names(&sections))?,
                None => s,
            };
            // Next to another task, the section is that task's. Counted without the moved one.
            let place = |other: &str| match find(other)? {
                Found::Task(other_s, other_t) if (other_s, other_t) == (s, t) => {
                    bail!("'{other}' is the task to move, it cannot go next to itself")
                }
                Found::Task(other_s, other_t) if args.to.is_none() || other_s == to => {
                    Ok((other_s, other_t - usize::from(other_s == s && t < other_t)))
                }
                Found::Task(..) => {
                    bail!("'{other}' is no task in section '{}'", sections[to].name)
                }
                Found::Section(_) => bail!("'{other}' is a section, --to moves a task into it"),
                Found::Archived(_) => bail!("'{other}' is archived, archived tasks are read-only"),
            };
            let (to, index) = match (&args.before, &args.after) {
                (Some(other), _) => place(other)?,
                (_, Some(other)) => place(other).map(|(to, index)| (to, index + 1))?,
                _ if args.top => (to, 0),
                _ => (to, sections[to].tasks.len() - usize::from(to == s)),
            };
            let (sections, file) = normalize(root)?;
            let (ops, path) = edit::move_task(&sections, s, t, to, index, &file);
            op::apply_all(&ops)?;
            Ok(format!("{}\n", id::of(root, &path)))
        }
    }
}

/// `D` on each id: tasks, and sections that are empty once the tasks are deleted.
pub fn delete(root: &Path, ids: &[String]) -> Result<String> {
    let mut paths = paths(root, ids, true)?;
    // All checks before the first change, so a refused delete changes nothing.
    let (sections, _) = load::load(root)?;
    for section in sections
        .iter()
        .filter(|section| paths.contains(&section.dir))
    {
        let left: Vec<_> = (section.tasks.iter())
            .filter(|task| !paths.contains(&task.path))
            .map(|task| id::of(root, &task.path))
            .collect();
        if !left.is_empty() {
            bail!(
                "'{}' is not empty, delete or move its tasks first: {}",
                section.name,
                left.join(", ")
            );
        }
    }
    // Tasks first, a section has to be empty.
    paths.sort_by_key(|path| sections.iter().any(|section| &section.dir == path));
    for path in paths {
        let (sections, file) = load::load(root)?;
        let ops = match position(&sections, &path) {
            Found::Section(s) => edit::delete_section(&sections, s, &file),
            Found::Task(s, t) => edit::delete(&sections, s, t, &file),
            Found::Archived(_) => unreachable!("paths only has open ones"),
        };
        op::apply_all(&ops)?;
    }
    Ok(String::new())
}

/// `x` on each task in turn, `X` with `cancel`. Prints where each went: its id in the archive,
/// or for a repeating one its id and when it is due again.
pub fn done(root: &Path, ids: &[String], cancel: bool, today: Date) -> Result<String> {
    let mut text = String::new();
    for path in paths(root, ids, false)? {
        let (sections, file) = load::load(root)?;
        let Found::Task(s, t) = position(&sections, &path) else {
            unreachable!("paths only has tasks")
        };
        let task = &sections[s].tasks[t];
        if let Some(repeat) = &task.repeat {
            let ops = edit::repeat_done(&sections, s, t, today, &file);
            op::apply_all(&ops)?;
            let due = match repeat.next(today) {
                Some(next) => format!("due {next}"),
                None => "never due".to_string(),
            };
            text.push_str(&format!("{} ({due})\n", id::of(root, &path)));
            continue;
        }
        let archive = load::archive(root)?;
        let to = load::archive_dir(root, today, &sections[s].name, &archive);
        let (ops, archived) = edit::archive(&sections, s, t, cancel, &to, &file);
        op::apply_all(&ops)?;
        text.push_str(&format!("{}\n", id::of(root, &archived)));
    }
    Ok(text)
}

/// The paths of `ids` after normalizing, so commands with several ids can find each one again
/// after the changes of the ones before. Only tasks, or sections too with `sections`.
fn paths(root: &Path, ids: &[String], sections: bool) -> Result<Vec<PathBuf>> {
    let (found, archive) = (load::load(root)?.0, load::archive(root)?);
    let mut targets = Vec::new();
    for id in ids {
        let target = match id::find(id, root, &found, &archive)? {
            Found::Archived(_) => bail!("'{id}' is archived, archived tasks are read-only"),
            Found::Section(_) if !sections => {
                bail!("'{id}' is a section, not a task, see 'markoff list {id}'")
            }
            target => target,
        };
        if !targets.contains(&target) {
            targets.push(target);
        }
    }
    let (normalized, _) = normalize(root)?;
    Ok(targets
        .into_iter()
        .map(|target| match target {
            Found::Section(s) => normalized[s].dir.clone(),
            Found::Task(s, t) => normalized[s].tasks[t].path.clone(),
            Found::Archived(_) => unreachable!("refused above"),
        })
        .collect())
}

/// Where the section or task at `path` is in `sections`. It is there, [`paths`] found it.
fn position(sections: &[Section], path: &Path) -> Found<'static> {
    for (s, section) in sections.iter().enumerate() {
        if section.dir == path {
            return Found::Section(s);
        }
        if let Some(t) = section.tasks.iter().position(|task| task.path == path) {
            return Found::Task(s, t);
        }
    }
    unreachable!("{} is gone", path.display())
}

/// Normalizes the root and loads it again.
fn normalize(root: &Path) -> Result<(Vec<Section>, StateFile)> {
    let (sections, file) = load::load(root)?;
    op::apply_all(&edit::normalize(&sections, &file))?;
    load::load(root)
}

/// The index of `found`, which `id` has to name a section.
fn section(id: &str, found: Found) -> Result<usize> {
    match found {
        Found::Section(s) => Ok(s),
        _ => bail!("'{id}' is a task, a section goes before or after another section"),
    }
}

/// The index of `found` in section `s` named `name`, which `id` has to name a task of.
fn task_in(id: &str, found: Found, s: usize, name: &str) -> Result<usize> {
    match found {
        Found::Task(task_s, t) if task_s == s => Ok(t),
        _ => bail!("'{id}' is no task in section '{name}'"),
    }
}

/// A task file of `title` as heading and `body` below it.
fn content(title: &str, body: &str) -> String {
    let body = body.trim_start_matches(['\r', '\n']).trim_end();
    match body.is_empty() {
        true => format!("{HEADING}{title}\n"),
        false => format!("{HEADING}{title}\n\n{body}\n"),
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use super::*;
    use crate::{
        app::tests::TODAY,
        cli::tests::{markoff, root},
        state,
    };

    /// The ids `list` shows, open tasks with `--hidden`, section by section.
    fn ids(root: &Path) -> String {
        let (sections, _) = load::load(root).unwrap();
        (sections.iter())
            .map(|section| {
                let tasks: Vec<_> = (section.tasks.iter())
                    .map(|task| task.stem().to_string())
                    .collect();
                format!("{}: {}", section.name, tasks.join(" "))
            })
            .collect::<Vec<_>>()
            .join(" | ")
    }

    fn read(root: &Path, path: &str) -> String {
        fs::read_to_string(root.join(path)).unwrap()
    }

    /// `markoff` with `args` that has to fail, its message.
    fn error(root: &Path, args: &[&str]) -> String {
        markoff(root, args).unwrap_err().to_string()
    }

    #[test]
    fn add_a_task() {
        let dir = root();
        let root = dir.path();
        assert_eq!(
            markoff(root, &["add", "work", " New thing "]).unwrap(),
            "work/new-thing\n"
        );
        assert_eq!(read(root, "work/new-thing.md"), "# New thing\n");
        assert_eq!(
            read(root, state::FILE),
            "# work\nfix-the-bug\ncall-bob\nnew-thing\n\n# home\n"
        );

        // A taken name gets a suffix, an empty section works too.
        assert_eq!(
            markoff(root, &["add", "work", "New thing"]).unwrap(),
            "work/new-thing-2\n"
        );
        markoff(root, &["add", "home", "Water plants `* * mon`"]).unwrap();
        assert_eq!(
            ids(root),
            "work: fix-the-bug call-bob new-thing new-thing-2 | home: water-plants"
        );
        assert_eq!(
            read(root, "home/water-plants.md"),
            "# Water plants `* * mon`\n"
        );

        // With a body, before or after a task.
        let args = [
            "add",
            "work",
            "First",
            "--before",
            "work/fix-the-bug",
            "--body",
            "\nnotes\n\n",
        ];
        markoff(root, &args).unwrap();
        assert_eq!(read(root, "work/first.md"), "# First\n\nnotes\n");
        markoff(root, &["add", "work", "Second", "--after", "work/first"]).unwrap();
        assert_eq!(
            ids(root),
            "work: first second fix-the-bug call-bob new-thing new-thing-2 | home: water-plants"
        );
    }

    #[test]
    fn add_normalizes_first() {
        let dir = root();
        let root = dir.path();
        fs::write(root.join("work/fix-the-bug.md"), "# Renamed\n").unwrap();
        // The id from before the rename still works.
        markoff(root, &["add", "work", "New", "--after", "work/fix-the-bug"]).unwrap();
        assert_eq!(ids(root), "work: renamed new call-bob | home: ");
    }

    #[test]
    fn add_a_section() {
        let dir = root();
        let root = dir.path();
        assert_eq!(
            markoff(root, &["add", "--section", "My Stuff"]).unwrap(),
            "my-stuff\n"
        );
        markoff(root, &["add", "--section", "first", "--before", "work"]).unwrap();
        markoff(root, &["add", "--section", "second", "--after", "first"]).unwrap();
        assert_eq!(
            ids(root),
            "first:  | second:  | work: fix-the-bug call-bob | home:  | my-stuff: "
        );
        assert!(root.join("my-stuff").is_dir());
    }

    #[test]
    fn add_errors_change_nothing() {
        let dir = root();
        let root = dir.path();
        // Needs normalizing, so a write would show as a rename.
        fs::write(root.join("work/fix-the-bug.md"), "# Renamed\n").unwrap();
        let error = |args: &[&str]| error(root, args);
        assert_eq!(
            error(&["add", "wrok", "New"]),
            "no section 'wrok', did you mean 'work'?"
        );
        assert_eq!(
            error(&["add", "errands", "New"]),
            "no section 'errands', the sections: work, home"
        );
        assert_eq!(error(&["add", "work", "  "]), "cannot add: empty title");
        assert_eq!(
            error(&["add", "home", "New", "--before", "work/call-bob"]),
            "'work/call-bob' is no task in section 'home'"
        );
        assert_eq!(
            error(&["add", "--section", "work"]),
            "a section named 'work' already exists"
        );
        assert_eq!(error(&["add", "--section", " "]), "cannot add: empty name");
        assert_eq!(
            error(&["add", "--section", "x", "--after", "work/call-bob"]),
            "'work/call-bob' is a task, a section goes before or after another section"
        );
        assert!(root.join("work/fix-the-bug.md").exists());

        let empty = TempDir::new().unwrap();
        assert_eq!(
            markoff(empty.path(), &["add", "work", "New"])
                .unwrap_err()
                .to_string(),
            "no section 'work', there are none yet"
        );
    }

    #[test]
    fn edit_a_task() {
        let dir = root();
        let root = dir.path();
        let edit = |args: &[&str]| markoff(root, &[&["edit"], args].concat()).unwrap();

        // A new title renames the file at once.
        assert_eq!(
            edit(&["work/fix-the-bug", "--title", "Fix it"]),
            "work/fix-it\n"
        );
        assert_eq!(read(root, "work/fix-it.md"), "# Fix it\n\nbody\n");
        assert_eq!(
            edit(&["work/fix-it", "--body", "new body"]),
            "work/fix-it\n"
        );
        assert_eq!(read(root, "work/fix-it.md"), "# Fix it\n\nnew body\n");
        edit(&["work/fix-it", "--append", "- [ ] more"]);
        assert_eq!(
            read(root, "work/fix-it.md"),
            "# Fix it\n\nnew body\n- [ ] more\n"
        );

        // The heading stays as written: with a rule, or struck through by hand.
        fs::write(root.join("home/water.md"), "# ~~Water~~ `* * *`\n").unwrap();
        edit(&["home/water", "--append", "notes"]);
        assert_eq!(
            read(root, "home/water.md"),
            "# ~~Water~~ `* * *`\n\nnotes\n"
        );

        // Without a heading it gets one from the filename.
        edit(&["work/call-bob", "--title", "Call Bob", "--body", "at 5"]);
        assert_eq!(read(root, "work/call-bob.md"), "# Call Bob\n\nat 5\n");

        // The id from before a rename by hand still works, the printed one is the new.
        fs::write(root.join("work/fix-it.md"), "# Renamed\n").unwrap();
        assert_eq!(edit(&["work/fix-it", "--append", "x"]), "work/renamed\n");
    }

    #[test]
    fn edit_a_section() {
        let dir = root();
        let root = dir.path();
        assert_eq!(
            markoff(root, &["edit", "work", "--name", "Job"]).unwrap(),
            "job\n"
        );
        assert_eq!(ids(root), "job: fix-the-bug call-bob | home: ");
        assert_eq!(
            markoff(root, &["edit", "job", "--name", "job"]).unwrap(),
            "job\n"
        );
    }

    #[test]
    fn edit_errors() {
        let dir = root();
        let root = dir.path();
        fs::create_dir_all(root.join(".archive/2026-10-01/work")).unwrap();
        fs::write(root.join(".archive/2026-10-01/work/old.md"), "# Old\n").unwrap();
        let error = |args: &[&str]| error(root, &[&["edit"], args].concat());
        assert_eq!(
            error(&["work", "--title", "x"]),
            "'work' is a section, only --name changes it"
        );
        assert_eq!(
            error(&["work", "--name", "x", "--body", "y"]),
            "'work' is a section, only --name changes it"
        );
        assert_eq!(
            error(&["work/call-bob", "--name", "x"]),
            "'work/call-bob' is a task, --name is for sections, --title renames a task"
        );
        assert_eq!(
            error(&["work/call-bob", "--title", " "]),
            "cannot edit: empty title"
        );
        assert_eq!(
            error(&["work", "--name", "home"]),
            "a section named 'home' already exists"
        );
        assert_eq!(
            error(&[".archive/2026-10-01/work/old", "--title", "x"]),
            "'.archive/2026-10-01/work/old' is archived, archived tasks are read-only"
        );
    }

    #[test]
    fn move_a_task() {
        let dir = root();
        let root = dir.path();
        fs::write(root.join("work/c.md"), "# C\n").unwrap();
        let moved = |args: &[&str]| {
            markoff(root, &[&["move"], args].concat()).unwrap();
            ids(root)
        };
        assert_eq!(
            moved(&["work/c", "--top"]),
            "work: c fix-the-bug call-bob | home: "
        );
        assert_eq!(
            moved(&["work/c", "--bottom"]),
            "work: fix-the-bug call-bob c | home: "
        );
        assert_eq!(
            moved(&["work/fix-the-bug", "--after", "work/call-bob"]),
            "work: call-bob fix-the-bug c | home: "
        );
        assert_eq!(
            moved(&["work/c", "--before", "work/fix-the-bug"]),
            "work: call-bob c fix-the-bug | home: "
        );

        // Into another section, at its end, or next to a task there.
        assert_eq!(
            markoff(root, &["move", "work/c", "--to", "home"]).unwrap(),
            "home/c\n"
        );
        assert_eq!(
            moved(&["work/call-bob", "--to", "home", "--top"]),
            "work: fix-the-bug | home: call-bob c"
        );
        assert_eq!(
            moved(&["work/fix-the-bug", "--after", "home/call-bob"]),
            "work:  | home: call-bob fix-the-bug c"
        );
    }

    #[test]
    fn move_a_section() {
        let dir = root();
        let root = dir.path();
        fs::create_dir(root.join("x")).unwrap();
        let moved = |args: &[&str]| {
            markoff(root, &[&["move"], args].concat()).unwrap();
            ids(root)
                .split(" | ")
                .map(|s| s.split(':').next().unwrap().to_string())
                .collect::<Vec<_>>()
        };
        assert_eq!(moved(&["x", "--top"]), ["x", "work", "home"]);
        assert_eq!(moved(&["x", "--bottom"]), ["work", "home", "x"]);
        assert_eq!(moved(&["work", "--after", "home"]), ["home", "work", "x"]);
        assert_eq!(moved(&["x", "--before", "work"]), ["home", "x", "work"]);
    }

    #[test]
    fn move_errors() {
        let dir = root();
        let root = dir.path();
        let error = |args: &[&str]| error(root, &[&["move"], args].concat());
        assert_eq!(
            error(&["work", "--to", "home"]),
            "'work' is a section, --to is for tasks, --before or --after moves it"
        );
        assert_eq!(
            error(&["work", "--after", "work"]),
            "'work' is the section to move, it cannot go next to itself"
        );
        assert_eq!(
            error(&["work/call-bob", "--after", "work/call-bob"]),
            "'work/call-bob' is the task to move, it cannot go next to itself"
        );
        assert_eq!(
            error(&["work/call-bob", "--after", "home"]),
            "'home' is a section, --to moves a task into it"
        );
        assert_eq!(
            error(&[
                "work/call-bob",
                "--to",
                "home",
                "--after",
                "work/fix-the-bug"
            ]),
            "'work/fix-the-bug' is no task in section 'home'"
        );
        assert_eq!(
            error(&["work", "--before", "work/call-bob"]),
            "'work/call-bob' is a task, a section goes before or after another section"
        );
    }

    #[test]
    fn delete_tasks_and_empty_sections() {
        let dir = root();
        let root = dir.path();
        assert_eq!(
            error(root, &["delete", "work/call-bob", "nope"]),
            "no section or task 'nope', see 'markoff list'"
        );
        assert_eq!(
            ids(root),
            "work: fix-the-bug call-bob | home: ",
            "nothing deleted"
        );
        assert_eq!(
            error(root, &["delete", "work"]),
            "'work' is not empty, delete or move its tasks first: work/fix-the-bug, work/call-bob"
        );
        assert_eq!(
            error(root, &["delete", "home", "work", "work/call-bob"]),
            "'work' is not empty, delete or move its tasks first: work/fix-the-bug"
        );
        assert_eq!(
            ids(root),
            "work: fix-the-bug call-bob | home: ",
            "nothing deleted"
        );
        assert_eq!(
            markoff(root, &["delete", "work/call-bob", "home"]).unwrap(),
            ""
        );
        assert_eq!(ids(root), "work: fix-the-bug");
        // A section with all its tasks, in any order.
        markoff(root, &["delete", "work", "work/fix-the-bug"]).unwrap();
        assert_eq!(ids(root), "");
    }

    #[test]
    fn done_and_cancel() {
        let dir = root();
        let root = dir.path();
        fs::write(root.join("home/water.md"), "# Water `* * mon`\n").unwrap();
        let day = format!(".archive/{TODAY}");

        assert_eq!(
            markoff(root, &["done", "work/fix-the-bug", "home/water"]).unwrap(),
            format!("{day}/work/fix-the-bug\nhome/water (due 2026-10-26)\n")
        );
        assert_eq!(
            read(root, &format!("{day}/work/fix-the-bug.md")),
            "# Fix the bug\n\nbody\n"
        );
        assert_eq!(
            read(root, state::FILE),
            format!("# work\ncall-bob\n\n# home\nwater {TODAY}\n")
        );

        assert_eq!(
            markoff(root, &["cancel", "work/call-bob"]).unwrap(),
            format!("{day}/work/call-bob\n")
        );
        assert_eq!(
            read(root, &format!("{day}/work/call-bob.md")),
            "# ~~call bob~~\n\nno heading\n"
        );

        assert_eq!(
            error(root, &["done", "home"]),
            "'home' is a section, not a task, see 'markoff list home'"
        );
        assert_eq!(
            error(root, &["done", &format!("{day}/work/call-bob")]),
            format!("'{day}/work/call-bob' is archived, archived tasks are read-only")
        );
    }
}
