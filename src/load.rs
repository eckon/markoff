//! Reads the `todo/` directory into [`Section`]s.

use std::{
    fs,
    path::{Path, PathBuf},
};

use color_eyre::{Result, eyre::WrapErr};

use crate::{
    name::Name,
    task::{Section, Task},
};

/// Loads all sections, inbox first, then the section directories in prefix order.
///
/// The inbox is always present, even if empty, so there is always somewhere to add a task.
/// Only one level of sections is read, deeper directories are ignored for now.
pub fn load(todo_dir: &Path) -> Result<Vec<Section>> {
    let entries = visible_entries(todo_dir)?;

    let mut inbox_tasks = Vec::new();
    let mut section_dirs = Vec::new();
    for path in entries {
        if path.is_dir() {
            section_dirs.push(path);
        } else if is_markdown(&path) {
            inbox_tasks.push(path);
        }
    }

    let mut sections = vec![Section {
        path: vec![],
        dir: todo_dir.to_path_buf(),
        tasks: load_tasks(inbox_tasks)?,
    }];

    sort_by_prefix(&mut section_dirs);
    for dir in section_dirs {
        let task_paths = visible_entries(&dir)?
            .into_iter()
            .filter(|path| path.is_file() && is_markdown(path))
            .collect();
        sections.push(Section {
            path: vec![section_name(&dir)],
            tasks: load_tasks(task_paths)?,
            dir,
        });
    }

    Ok(sections)
}

fn load_tasks(mut paths: Vec<PathBuf>) -> Result<Vec<Task>> {
    sort_by_prefix(&mut paths);
    paths
        .into_iter()
        .map(|path| {
            let content = fs::read_to_string(&path)
                .wrap_err_with(|| format!("failed to read {}", path.display()))?;
            Ok(Task::parse(path, &content))
        })
        .collect()
}

/// Directory entries without hidden ones (`.git`, `.DS_Store`, editor swap files).
fn visible_entries(dir: &Path) -> Result<Vec<PathBuf>> {
    let read_dir =
        fs::read_dir(dir).wrap_err_with(|| format!("failed to read {}", dir.display()))?;

    let mut paths = Vec::new();
    for entry in read_dir {
        let path = entry?.path();
        if !file_name(&path).starts_with('.') {
            paths.push(path);
        }
    }
    Ok(paths)
}

/// Sorts by prefix. Entries without a prefix go last, sorted by name.
fn sort_by_prefix(paths: &mut [PathBuf]) {
    paths.sort_by_cached_key(|path| {
        let name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .and_then(Name::parse);
        let order = name.map(|n| n.order);
        (order.is_none(), order, file_name(path).to_string())
    });
}

/// `010-work` -> `work`, a directory without prefix keeps its full name.
fn section_name(dir: &Path) -> String {
    let dir_name = file_name(dir);
    Name::parse(dir_name).map_or_else(|| dir_name.to_string(), |name| name.slug)
}

fn file_name(path: &Path) -> &str {
    path.file_name()
        .and_then(|s| s.to_str())
        .unwrap_or_default()
}

fn is_markdown(path: &Path) -> bool {
    path.extension().is_some_and(|ext| ext == "md")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    /// Creates a temp `todo/` with the given files. Paths ending in `/` become empty directories.
    fn fixture(files: &[(&str, &str)]) -> TempDir {
        let dir = TempDir::new().unwrap();
        for (path, content) in files {
            let path = dir.path().join(path);
            if path.to_str().unwrap().ends_with('/') {
                fs::create_dir_all(&path).unwrap();
                continue;
            }
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, content).unwrap();
        }
        dir
    }

    fn titles(section: &Section) -> Vec<String> {
        section.tasks.iter().map(Task::title).collect()
    }

    #[test]
    fn empty_dir_has_inbox() {
        let dir = fixture(&[]);
        let sections = load(dir.path()).unwrap();
        assert_eq!(sections.len(), 1);
        assert_eq!(sections[0].title(), "inbox");
        assert!(sections[0].tasks.is_empty());
    }

    #[test]
    fn sections_and_tasks_in_prefix_order() {
        let dir = fixture(&[
            ("010-fix-typo.md", "# Fix typo"),
            ("020-private/010-call-bob.md", "# Call Bob"),
            ("010-work/020-review-pr.md", "# Review PR"),
            ("010-work/010-fix-login-bug.md", "# Fix login bug"),
        ]);
        let sections = load(dir.path()).unwrap();

        let section_titles: Vec<_> = sections.iter().map(Section::title).collect();
        assert_eq!(section_titles, ["inbox", "work", "private"]);
        assert_eq!(titles(&sections[0]), ["Fix typo"]);
        assert_eq!(titles(&sections[1]), ["Fix login bug", "Review PR"]);
        assert_eq!(titles(&sections[2]), ["Call Bob"]);
    }

    #[test]
    fn unprefixed_go_last() {
        let dir = fixture(&[
            ("notes.md", ""),
            ("020-b.md", ""),
            ("010-a.md", ""),
            ("misc/", ""),
            ("010-work/", ""),
        ]);
        let sections = load(dir.path()).unwrap();

        assert_eq!(titles(&sections[0]), ["a", "b", "notes"]);
        let section_titles: Vec<_> = sections.iter().map(Section::title).collect();
        assert_eq!(section_titles, ["inbox", "work", "misc"]);
    }

    #[test]
    fn ignores_hidden_non_markdown_and_nested() {
        let dir = fixture(&[
            (".git/config", ""),
            (".hidden.md", ""),
            ("readme.txt", ""),
            ("010-work/.010-swap.md", ""),
            ("010-work/010-nested/010-deep.md", ""),
            ("010-work/010-task.md", "# Task"),
        ]);
        let sections = load(dir.path()).unwrap();

        assert_eq!(sections.len(), 2);
        assert!(sections[0].tasks.is_empty());
        assert_eq!(titles(&sections[1]), ["Task"]);
    }

    #[test]
    fn missing_dir_is_an_error() {
        let dir = fixture(&[]);
        let err = load(&dir.path().join("nope")).unwrap_err();
        assert!(err.to_string().contains("failed to read"));
    }
}
