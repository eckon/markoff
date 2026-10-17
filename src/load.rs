//! Reads the `todo/` directory into [`Section`]s, in the order of [`order::FILE`].

use std::{
    fs, io,
    path::{Path, PathBuf},
};

use color_eyre::{Result, eyre::WrapErr};

use crate::{
    order::{self, Order, OrderFile},
    task::{Section, Task},
};

/// Loads all sections, inbox first, then the section directories, both in the order file's
/// order. What it does not list goes last, sorted by name.
///
/// Also returns the order file as read. Actions write it based on that content, so it is
/// never overwritten with an order from before a change made outside markoff.
///
/// The inbox is always present, even if empty, so there is always somewhere to add a task.
/// Only one level of sections is read, deeper directories are ignored for now.
pub fn load(todo_dir: &Path) -> Result<(Vec<Section>, OrderFile)> {
    let file = read_order_file(todo_dir)?;
    // A missing order file is an empty order: everything sorted by name.
    let order = file
        .content
        .as_deref()
        .map(Order::parse)
        .unwrap_or_default();
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
        tasks: load_tasks(inbox_tasks, &order, "")?,
    }];

    sort_by_order(&mut section_dirs, |dir| {
        order.section_position(file_name(dir))
    });
    for dir in section_dirs {
        let name = file_name(&dir).to_string();
        let task_paths = visible_entries(&dir)?
            .into_iter()
            .filter(|path| path.is_file() && is_markdown(path))
            .collect();
        sections.push(Section {
            tasks: load_tasks(task_paths, &order, &name)?,
            path: vec![name],
            dir,
        });
    }

    Ok((sections, file))
}

fn read_order_file(todo_dir: &Path) -> Result<OrderFile> {
    let path = todo_dir.join(order::FILE);
    let content = match fs::read_to_string(&path) {
        Ok(content) => Some(content),
        Err(err) if err.kind() == io::ErrorKind::NotFound => None,
        Err(err) => {
            return Err(err).wrap_err_with(|| format!("failed to read {}", path.display()));
        }
    };
    Ok(OrderFile { path, content })
}

fn load_tasks(mut paths: Vec<PathBuf>, order: &Order, section: &str) -> Result<Vec<Task>> {
    sort_by_order(&mut paths, |path| {
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or_default();
        order.task_position(section, stem)
    });
    paths
        .into_iter()
        .map(|path| {
            let content = fs::read_to_string(&path)
                .wrap_err_with(|| format!("failed to read {}", path.display()))?;
            Ok(Task::parse(path, &content))
        })
        .collect()
}

/// Directory entries without hidden ones (`.git`, `.order`, editor swap files).
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

/// Sorts by `position`. Entries without one go last, sorted by name.
fn sort_by_order(paths: &mut [PathBuf], position: impl Fn(&Path) -> Option<usize>) {
    paths.sort_by_cached_key(|path| {
        let position = position(path);
        (position.is_none(), position, file_name(path).to_string())
    });
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
        let (sections, _) = load(dir.path()).unwrap();
        assert_eq!(sections.len(), 1);
        assert_eq!(sections[0].title(), "inbox");
        assert!(sections[0].tasks.is_empty());
    }

    fn section_titles(sections: &[Section]) -> Vec<String> {
        sections.iter().map(Section::title).collect()
    }

    #[test]
    fn sections_and_tasks_in_order_file_order() {
        let dir = fixture(&[
            (
                ".order",
                "fix-typo\n\n# work\nreview-pr\nfix-login-bug\n\n# private\n",
            ),
            ("fix-typo.md", "# Fix typo"),
            ("private/call-bob.md", "# Call Bob"),
            ("work/fix-login-bug.md", "# Fix login bug"),
            ("work/review-pr.md", "# Review PR"),
        ]);
        let (sections, _) = load(dir.path()).unwrap();

        assert_eq!(section_titles(&sections), ["inbox", "work", "private"]);
        assert_eq!(titles(&sections[0]), ["Fix typo"]);
        assert_eq!(titles(&sections[1]), ["Review PR", "Fix login bug"]);
        assert_eq!(
            titles(&sections[2]),
            ["Call Bob"],
            "not listed, still loaded"
        );
    }

    #[test]
    fn unlisted_go_last_by_name_stale_are_ignored() {
        let dir = fixture(&[
            (".order", "b\ngone\n# work\n# gone\n"),
            ("c.md", ""),
            ("a.md", ""),
            ("b.md", ""),
            ("misc/", ""),
            ("home/", ""),
            ("work/", ""),
        ]);
        let (sections, _) = load(dir.path()).unwrap();

        assert_eq!(titles(&sections[0]), ["b", "a", "c"]);
        assert_eq!(section_titles(&sections), ["inbox", "work", "home", "misc"]);
    }

    #[test]
    fn without_order_file_by_name() {
        // Old numbered names keep their order this way until they are renamed.
        let dir = fixture(&[("02-b.md", ""), ("01-a.md", ""), ("01-work/", "")]);
        let (sections, _) = load(dir.path()).unwrap();
        assert_eq!(titles(&sections[0]), ["01 a", "02 b"]);
        assert_eq!(section_titles(&sections), ["inbox", "01-work"]);
    }

    #[test]
    fn ignores_hidden_non_markdown_and_nested() {
        let dir = fixture(&[
            (".git/config", ""),
            (".hidden.md", ""),
            ("readme.txt", ""),
            ("work/.swap.md", ""),
            ("work/nested/deep.md", ""),
            ("work/task.md", "# Task"),
        ]);
        let (sections, _) = load(dir.path()).unwrap();

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
