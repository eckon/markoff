//! Ids, how commands name sections and tasks. A section's id is its name, `work`. A task's id
//! is its path in the root without `.md`, `work/fix-the-bug`, archived ones included:
//! `.archive/2026-11-05/work/fix-the-bug`. Unlike titles, ids are unique.
//!
//! A wrong id is an error that says how to find the right one, so a script or an agent can fix
//! its call without help.

use std::path::Path;

use color_eyre::{Result, eyre::bail};

use crate::{
    archive::{self, Archive},
    task::{EXTENSION, Section, Task},
};

/// The id of the task at `path`, open or archived.
pub fn of(root: &Path, path: &Path) -> String {
    let path = path.strip_prefix(root).unwrap_or(path);
    path.with_extension("").display().to_string()
}

/// What an id names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Found<'a> {
    Section(usize),
    /// Task `t` of section `s`, as `(s, t)`.
    Task(usize, usize),
    Archived(&'a Task),
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

/// The section or task with `id`, open or archived. Also takes a task's id with `.md`, or as
/// a path that starts with the root, as other tools print it.
pub fn find<'a>(
    id: &str,
    root: &Path,
    sections: &'a [Section],
    archive: &'a Archive,
) -> Result<Found<'a>> {
    let id = id.trim().trim_end_matches('/');
    let id = id
        .strip_prefix(&format!("{}/", root.display()))
        .unwrap_or(id);
    let id = id.strip_prefix("./").unwrap_or(id);
    let id = id.strip_suffix(&format!(".{EXTENSION}")).unwrap_or(id);

    let mut ids = Vec::new();
    for (s, section) in sections.iter().enumerate() {
        ids.push((section.name.clone(), Found::Section(s)));
        for (t, task) in section.tasks.iter().enumerate() {
            ids.push((of(root, &task.path), Found::Task(s, t)));
        }
    }
    for done in archive.values().flatten() {
        ids.push((of(root, &done.task.path), Found::Archived(&done.task)));
    }
    if let Some((_, found)) = ids.iter().find(|(other, _)| other == id) {
        return Ok(*found);
    }

    // No suggestion in the archive: its ids share so much that any would count as close, and
    // the closest is mostly a task of another day.
    if id.starts_with(archive::DIR) {
        bail!("no task '{id}', see 'markoff list --archive'");
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
    use crate::{cli::tests::root, load};

    #[test]
    fn find_by_id() {
        let dir = root();
        let root = dir.path();
        fs::create_dir_all(root.join(".archive/2026-11-05/work")).unwrap();
        fs::write(root.join(".archive/2026-11-05/work/old.md"), "# Old\n").unwrap();
        let (sections, _) = load::load(root).unwrap();
        let archive = load::archive(root).unwrap();
        let find = |id: &str| find(id, root, &sections, &archive).map_err(|err| err.to_string());

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
        let Ok(Found::Archived(old)) = find(".archive/2026-11-05/work/old") else {
            panic!("archived task not found")
        };
        assert_eq!(old.title(), "Old");

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
        assert_eq!(
            error(".archive/x"),
            "no task '.archive/x', see 'markoff list --archive'"
        );
        // Hidden and root files are no tasks, even though they exist.
        fs::write(root.join("notes.md"), "").unwrap();
        assert!(find("notes").is_err());
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
