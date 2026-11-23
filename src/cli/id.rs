//! Ids, how commands name sections and tasks. A section's id is its name, `work`. A task's id
//! is its path in the root without `.md`, `work/fix-the-bug`, archived ones included:
//! `.archive/2026-11-05/work/fix-the-bug`. Unlike titles, ids are unique.
//!
//! A wrong id is an error that says how to find the right one, so a script or an agent can fix
//! its call without help.

use std::path::Path;

use color_eyre::{Result, eyre::bail};
use jiff::civil::Date;

use crate::{
    archive, load,
    task::{EXTENSION, Section, Task},
};

/// The id of the task at `path`, open or archived.
pub fn of(root: &Path, path: &Path) -> String {
    let path = path.strip_prefix(root).unwrap_or(path);
    path.with_extension("").display().to_string()
}

/// What an id names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Found {
    Section(usize),
    /// Task `t` of section `s`, as `(s, t)`.
    Task(usize, usize),
}

/// The index of `name` in the section `names`.
pub fn section(name: &str, names: &[&str]) -> Result<usize> {
    if let Some(s) = names.iter().position(|section| *section == name) {
        return Ok(s);
    }
    match closest(name, names) {
        Some(close) => bail!("no section '{name}', did you mean '{close}'?"),
        None if names.is_empty() => bail!("no section '{name}', there are none yet"),
        None => bail!("no section '{name}', the sections: {}", names.join(", ")),
    }
}

/// The names of `sections`, for [`section`].
pub fn names(sections: &[Section]) -> Vec<&str> {
    sections
        .iter()
        .map(|section| section.name.as_str())
        .collect()
}

/// `id` without what other tools add to it: the root it starts with, `./`, `.md`, a trailing `/`.
fn clean<'a>(id: &'a str, root: &Path) -> &'a str {
    let id = id.trim().trim_end_matches('/');
    let id = id
        .strip_prefix(&format!("{}/", root.display()))
        .unwrap_or(id);
    let id = id.strip_prefix("./").unwrap_or(id);
    id.strip_suffix(&format!(".{EXTENSION}")).unwrap_or(id)
}

/// Whether `id` is in the archive, without reading it.
fn in_archive(id: &str) -> bool {
    id.split('/').next() == Some(archive::DIR)
}

/// The open section or task with `id`. Also takes a task's id with `.md`, or as a path that
/// starts with the root, as other tools print it. An archived id is refused without reading
/// the archive, archived tasks are read-only.
pub fn find(id: &str, root: &Path, sections: &[Section]) -> Result<Found> {
    let id = clean(id, root);
    if in_archive(id) {
        bail!("'{id}' is archived, archived tasks are read-only");
    }
    let mut ids = Vec::new();
    for (s, section) in sections.iter().enumerate() {
        ids.push((section.name.clone(), Found::Section(s)));
        for (t, task) in section.tasks.iter().enumerate() {
            ids.push((of(root, &task.path), Found::Task(s, t)));
        }
    }
    if let Some((_, found)) = ids.iter().find(|(other, _)| other == id) {
        return Ok(*found);
    }

    // Compared with its own kind, a task within its section if that exists: the shared start
    // would make `work/nope` close to any task in `work`.
    let (section, stem) = id.split_once('/').unwrap_or_default();
    let close = match sections.iter().find(|other| other.name == section) {
        Some(section) => {
            let stems: Vec<_> = section.tasks.iter().map(|task| task.stem()).collect();
            closest(stem, &stems).map(|stem| format!("{}/{stem}", section.name))
        }
        None => {
            let ids: Vec<_> = (ids.iter())
                .filter(|(_, found)| matches!(found, Found::Task(..)) == id.contains('/'))
                .map(|(other, _)| other.as_str())
                .collect();
            closest(id, &ids).map(str::to_string)
        }
    };
    match close {
        Some(close) => bail!("no section or task '{id}', did you mean '{close}'?"),
        None => bail!("no section or task '{id}', see 'markoff list'"),
    }
}

/// The archived task with `id`, `None` for an id outside the archive. Reads only its day.
pub fn archived(id: &str, root: &Path) -> Result<Option<Task>> {
    let id = clean(id, root);
    if !in_archive(id) {
        return Ok(None);
    }
    // No suggestion: archived ids share so much that any would count as close, and the closest
    // is mostly a task of another day.
    let missing = || format!("no task '{id}', see 'markoff list --archive'");
    let Some(day) = id
        .split('/')
        .nth(1)
        .and_then(|day| day.parse::<Date>().ok())
    else {
        bail!(missing());
    };
    let done = load::archive_day(root, day)?.unwrap_or_default();
    match done
        .into_iter()
        .find(|done| of(root, &done.task.path) == id)
    {
        Some(done) => Ok(Some(done.task)),
        None => bail!(missing()),
    }
}

/// The candidate most like `name`, if one is close enough to be a typo. Same measure and
/// threshold as clap's "a similar subcommand exists".
fn closest<'a>(name: &str, candidates: &[&'a str]) -> Option<&'a str> {
    candidates
        .iter()
        .map(|candidate| (strsim::jaro(name, candidate), *candidate))
        .filter(|(similarity, _)| *similarity > 0.7)
        .max_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, candidate)| candidate)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::cli::tests::root;

    #[test]
    fn find_by_id() {
        let dir = root();
        let root = dir.path();
        let (sections, _) = load::load(root).unwrap();
        let find = |id: &str| find(id, root, &sections).map_err(|err| err.to_string());

        let fix = Ok(Found::Task(0, 0));
        assert_eq!(find("work/fix-the-bug"), fix);
        assert_eq!(find(" work/fix-the-bug.md "), fix);
        assert_eq!(find("./work/fix-the-bug"), fix);
        assert_eq!(
            find(&root.join("work/fix-the-bug.md").display().to_string()),
            fix
        );
        assert_eq!(find("home"), Ok(Found::Section(1)));
        assert_eq!(find("work/"), Ok(Found::Section(0)));

        let error = |id| find(id).unwrap_err();
        assert_eq!(
            error("work/fix-teh-bug"),
            "no section or task 'work/fix-teh-bug', did you mean 'work/fix-the-bug'?"
        );
        assert_eq!(
            error("hom"),
            "no section or task 'hom', did you mean 'home'?"
        );
        assert_eq!(
            error("work/nope"),
            "no section or task 'work/nope', see 'markoff list'"
        );
        assert_eq!(
            error("shopping/milk"),
            "no section or task 'shopping/milk', see 'markoff list'"
        );
        // Refused without looking, there is no archive at all.
        assert_eq!(
            error(".archive/x"),
            "'.archive/x' is archived, archived tasks are read-only"
        );
        // Hidden and root files are no tasks, even though they exist.
        fs::write(root.join("notes.md"), "").unwrap();
        assert!(find("notes").is_err());
    }

    #[test]
    fn archived_by_id() {
        let dir = root();
        let root = dir.path();
        fs::create_dir_all(root.join(".archive/2026-11-05/work")).unwrap();
        fs::write(root.join(".archive/2026-11-05/work/old.md"), "# Old\n").unwrap();
        let archived = |id: &str| archived(id, root).map_err(|err| err.to_string());

        let old = archived("./.archive/2026-11-05/work/old.md")
            .unwrap()
            .unwrap();
        assert_eq!(old.title(), "Old");
        assert_eq!(archived("work/fix-the-bug"), Ok(None), "not in the archive");
        for id in [".archive/x", ".archive/2026-11-06/work/old"] {
            assert_eq!(
                archived(id),
                Err(format!("no task '{id}', see 'markoff list --archive'"))
            );
        }
    }

    #[test]
    fn closest_only_for_typos() {
        let names = ["work", "home", "errands"];
        assert_eq!(closest("wrok", &names), Some("work"));
        assert_eq!(closest("hom", &names), Some("home"));
        assert_eq!(closest("errand", &names), Some("errands"));
        assert_eq!(closest("shopping", &names), None);
        assert_eq!(closest("work", &[]), None);
    }
}
