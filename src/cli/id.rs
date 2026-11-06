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

/// The id of `task`, open or archived.
pub fn of(root: &Path, task: &Task) -> String {
    let path = task.path.strip_prefix(root).unwrap_or(&task.path);
    path.with_extension("").display().to_string()
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

/// The task with `id`, open or archived. Also takes the id with `.md`, or as a path that
/// starts with the root, as other tools print it.
pub fn task<'a>(
    id: &str,
    root: &Path,
    sections: &'a [Section],
    archive: &'a Archive,
) -> Result<&'a Task> {
    let id = id.trim().trim_end_matches('/');
    let id = id
        .strip_prefix(&format!("{}/", root.display()))
        .unwrap_or(id);
    let id = id.strip_prefix("./").unwrap_or(id);
    let id = id.strip_suffix(&format!(".{EXTENSION}")).unwrap_or(id);

    let open = sections.iter().flat_map(|section| &section.tasks);
    let archived = archive.values().flatten().map(|done| &done.task);
    let tasks: Vec<(String, &Task)> = open
        .chain(archived)
        .map(|task| (of(root, task), task))
        .collect();
    if let Some((_, task)) = tasks.iter().find(|(task_id, _)| task_id == id) {
        return Ok(task);
    }

    if sections.iter().any(|section| section.name == id) {
        bail!("'{id}' is a section, not a task, see 'markoff list {id}'");
    }
    // No suggestion in the archive: its ids share so much that any would count as close, and
    // the closest is mostly a task of another day.
    if id.starts_with(archive::DIR) {
        bail!("no task '{id}', see 'markoff list --archive'");
    }
    let ids: Vec<_> = tasks.iter().map(|(task_id, _)| task_id.as_str()).collect();
    match closest(id, &ids) {
        Some(close) => bail!("no task '{id}', did you mean '{close}'?"),
        None => bail!("no task '{id}', see 'markoff list'"),
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
    fn task_by_id() {
        let dir = root();
        let root = dir.path();
        fs::create_dir_all(root.join(".archive/2026-11-05/work")).unwrap();
        fs::write(root.join(".archive/2026-11-05/work/old.md"), "# Old\n").unwrap();
        let (sections, _) = load::load(root).unwrap();
        let archive = load::archive(root).unwrap();
        let find = |id: &str| {
            task(id, root, &sections, &archive)
                .map(|task| of(root, task))
                .map_err(|err| err.to_string())
        };

        let fix = Ok("work/fix-the-bug".to_string());
        assert_eq!(find("work/fix-the-bug"), fix);
        assert_eq!(find(" work/fix-the-bug.md "), fix);
        assert_eq!(find("./work/fix-the-bug"), fix);
        assert_eq!(
            find(&root.join("work/fix-the-bug.md").display().to_string()),
            fix
        );
        assert_eq!(
            find(".archive/2026-11-05/work/old"),
            Ok(".archive/2026-11-05/work/old".to_string())
        );

        let error = |id| find(id).unwrap_err();
        assert_eq!(
            error("work/fix-teh-bug"),
            "no task 'work/fix-teh-bug', did you mean 'work/fix-the-bug'?"
        );
        assert_eq!(
            error("work/"),
            "'work' is a section, not a task, see 'markoff list work'"
        );
        assert_eq!(
            error("shopping/milk"),
            "no task 'shopping/milk', see 'markoff list'"
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
