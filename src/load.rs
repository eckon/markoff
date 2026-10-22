//! Reads the root directory into [`Section`]s, in the order of [`state::FILE`], and the archive
//! into the done tasks per day.

use std::{
    fs, io,
    path::{Path, PathBuf},
};

use color_eyre::{Result, eyre::WrapErr};
use jiff::civil::Date;

use crate::{
    archive, history,
    state::{self, State, StateFile},
    task::{self, Section, Task},
};

/// Loads the sections and their tasks in the state file's order. Also returns the state file
/// as read, so actions never write over a change made outside markoff.
pub fn load(root: &Path) -> Result<(Vec<Section>, StateFile)> {
    let path = root.join(state::FILE);
    let file = StateFile {
        content: read_if_exists(&path)?,
        path,
    };
    // A missing state file is an empty order: everything sorted by name.
    let state = file
        .content
        .as_deref()
        .map(State::parse)
        .unwrap_or_default();

    let mut section_dirs: Vec<PathBuf> = visible_entries(root)?
        .into_iter()
        .filter(|path| path.is_dir())
        .collect();
    sort_by_order(&mut section_dirs, |dir| {
        state.section_position(file_name(dir))
    });

    let mut sections = Vec::new();
    for dir in section_dirs {
        let name = file_name(&dir).to_string();
        let task_paths = visible_entries(&dir)?
            .into_iter()
            .filter(|path| path.is_file() && is_markdown(path))
            .collect();
        sections.push(Section {
            tasks: load_tasks(task_paths, &state, &name)?,
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

/// The done tasks per day, from the archive files in `root`, see [`archive::tasks`]. Files whose
/// name is no date are skipped. No `.archive/` yet means nothing was done.
pub fn archive(root: &Path) -> Result<history::Archive> {
    let dir = root.join(archive::DIR);
    if !dir.is_dir() {
        return Ok(history::Archive::new());
    }
    let mut days = history::Archive::new();
    for path in visible_entries(&dir)? {
        let Some(date) = Some(&path)
            .filter(|path| is_markdown(path))
            .and_then(|path| path.file_stem()?.to_str())
            .and_then(|stem| stem.parse::<Date>().ok())
        else {
            continue;
        };
        if let Some(content) = read_if_exists(&path)? {
            days.insert(date, archive::tasks(&content));
        }
    }
    Ok(days)
}

fn load_tasks(mut paths: Vec<PathBuf>, state: &State, section: &str) -> Result<Vec<Task>> {
    sort_by_order(&mut paths, |path| {
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or_default();
        state.task_position(section, stem)
    });
    paths
        .into_iter()
        .map(|path| {
            let content = fs::read_to_string(&path)
                .wrap_err_with(|| format!("failed to read {}", path.display()))?;
            let mut task = Task::parse(path, &content);
            task.done = state.done(section, task.stem());
            Ok(task)
        })
        .collect()
}

/// Directory entries without hidden ones (`.git`, `.markoff`, `.archive`, editor swap files).
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
    path.extension().is_some_and(|ext| ext == task::EXTENSION)
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

    #[test]
    fn archive_by_date_skips_other_files() {
        let dir = fixture(&[
            (
                ".archive/2026-10-14.md",
                "# 2026-10-14\n\n## work\n\n### Fix bug\n",
            ),
            (".archive/2026-10-02.md", "# 2026-10-02\n"),
            (".archive/notes.md", "## work\n\n### Not a day\n"),
        ]);
        let days = archive(dir.path()).unwrap();
        let dates: Vec<String> = days.keys().map(Date::to_string).collect();
        assert_eq!(dates, ["2026-10-02", "2026-10-14"]);
        let fix_bug = archive::Done {
            section: "work".to_string(),
            title: "Fix bug".to_string(),
            body: String::new(),
        };
        assert_eq!(days[&"2026-10-14".parse().unwrap()], [fix_bug]);
    }

    #[test]
    fn archive_without_dir_is_empty() {
        let dir = fixture(&[]);
        assert!(archive(dir.path()).unwrap().is_empty());
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
    fn sections_and_tasks_in_state_file_order() {
        let dir = fixture(&[
            (
                ".markoff",
                "# work\nreview-pr\nfix-login-bug\n\n# private\n",
            ),
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
            (".markoff", "# work\nb\ngone\n# gone\n"),
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
    fn without_state_file_by_name() {
        // Old numbered names keep their order this way until they are renamed.
        let dir = fixture(&[("work/02-b.md", ""), ("work/01-a.md", ""), ("01-home/", "")]);
        let (sections, _) = load(dir.path()).unwrap();
        assert_eq!(section_titles(&sections), ["01-home", "work"]);
        assert_eq!(titles(&sections[1]), ["01 a", "02 b"]);
    }

    #[test]
    fn ignores_root_files_hidden_non_markdown_and_nested() {
        let dir = fixture(&[
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
