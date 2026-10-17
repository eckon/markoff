//! Reads the root directory into [`Section`]s, in the order of [`order::FILE`].

use std::{
    fs, io,
    path::{Path, PathBuf},
};

use color_eyre::{Result, eyre::WrapErr};

use crate::{
    order::{self, Order, OrderFile},
    task::{Section, Task},
};

/// Loads the section directories with their tasks, both in the order file's order. What it
/// does not list goes last, sorted by name.
///
/// Files directly in the root are not tasks, so notes or the daily checklist can live there.
/// Only one level of sections is read, deeper directories are ignored for now.
///
/// Also returns the order file as read. Actions write it based on that content, so it is
/// never overwritten with an order from before a change made outside markoff.
pub fn load(root: &Path) -> Result<(Vec<Section>, OrderFile)> {
    let path = root.join(order::FILE);
    let file = OrderFile {
        content: read_if_exists(&path)?,
        path,
    };
    // A missing order file is an empty order: everything sorted by name.
    let order = file
        .content
        .as_deref()
        .map(Order::parse)
        .unwrap_or_default();

    let mut section_dirs: Vec<PathBuf> = visible_entries(root)?
        .into_iter()
        .filter(|path| path.is_dir())
        .collect();
    sort_by_order(&mut section_dirs, |dir| {
        order.section_position(file_name(dir))
    });

    let mut sections = Vec::new();
    for dir in section_dirs {
        let name = file_name(&dir).to_string();
        let task_paths = visible_entries(&dir)?
            .into_iter()
            .filter(|path| path.is_file() && is_markdown(path))
            .collect();
        sections.push(Section {
            tasks: load_tasks(task_paths, &order, &name)?,
            name,
            dir,
        });
    }

    Ok((sections, file))
}

/// File content, `None` if the file does not exist.
pub fn read_if_exists(path: &Path) -> Result<Option<String>> {
    match fs::read_to_string(path) {
        Ok(content) => Ok(Some(content)),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(err).wrap_err_with(|| format!("failed to read {}", path.display())),
    }
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

/// Directory entries without hidden ones (`.git`, `.order`, `.archive`, editor swap files).
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

    /// Creates a temp root with the given files. Paths ending in `/` become empty directories.
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

    fn section_titles(sections: &[Section]) -> Vec<String> {
        sections
            .iter()
            .map(|section| section.name.clone())
            .collect()
    }

    #[test]
    fn empty_dir_has_no_sections() {
        let dir = fixture(&[]);
        let (sections, _) = load(dir.path()).unwrap();
        assert!(sections.is_empty());
    }

    #[test]
    fn sections_and_tasks_in_order_file_order() {
        let dir = fixture(&[
            (".order", "# work\nreview-pr\nfix-login-bug\n\n# private\n"),
            ("private/call-bob.md", "# Call Bob"),
            ("work/fix-login-bug.md", "# Fix login bug"),
            ("work/review-pr.md", "# Review PR"),
            ("home/water.md", "# Water plants"),
        ]);
        let (sections, _) = load(dir.path()).unwrap();

        assert_eq!(section_titles(&sections), ["work", "private", "home"]);
        assert_eq!(titles(&sections[0]), ["Review PR", "Fix login bug"]);
        assert_eq!(
            titles(&sections[2]),
            ["Water plants"],
            "not listed, still loaded"
        );
    }

    #[test]
    fn unlisted_go_last_by_name_stale_are_ignored() {
        let dir = fixture(&[
            (".order", "# work\nb\ngone\n# gone\n"),
            ("work/c.md", ""),
            ("work/a.md", ""),
            ("work/b.md", ""),
            ("misc/", ""),
            ("home/", ""),
        ]);
        let (sections, _) = load(dir.path()).unwrap();

        assert_eq!(section_titles(&sections), ["work", "home", "misc"]);
        assert_eq!(titles(&sections[0]), ["b", "a", "c"]);
    }

    #[test]
    fn without_order_file_by_name() {
        // Old numbered names keep their order this way until they are renamed.
        let dir = fixture(&[("work/02-b.md", ""), ("work/01-a.md", ""), ("01-home/", "")]);
        let (sections, _) = load(dir.path()).unwrap();
        assert_eq!(section_titles(&sections), ["01-home", "work"]);
        assert_eq!(titles(&sections[1]), ["01 a", "02 b"]);
    }

    #[test]
    fn ignores_root_files_hidden_non_markdown_and_nested() {
        let dir = fixture(&[
            ("daily.md", "## Jira"),
            ("readme.md", "# Notes"),
            (".git/config", ""),
            (".archive/2026-10-14.md", ""),
            ("work/.swap.md", ""),
            ("work/readme.txt", ""),
            ("work/nested/deep.md", ""),
            ("work/task.md", "# Task"),
        ]);
        let (sections, _) = load(dir.path()).unwrap();

        assert_eq!(section_titles(&sections), ["work"]);
        assert_eq!(titles(&sections[0]), ["Task"]);
    }

    #[test]
    fn missing_dir_is_an_error() {
        let dir = fixture(&[]);
        let err = load(&dir.path().join("nope")).unwrap_err();
        assert!(err.to_string().contains("failed to read"));
    }
}
