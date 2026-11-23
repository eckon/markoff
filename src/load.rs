//! Reads the root directory into [`Section`]s, in the order of [`state::FILE`], and the archive
//! into the done tasks per day.

use std::{
    fs, io,
    ops::RangeBounds,
    path::{Path, PathBuf},
};

use color_eyre::{Result, eyre::WrapErr};
use jiff::civil::Date;

use crate::{
    archive,
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

    let sections = sections(root, &state)?;
    Ok((sections, file))
}

/// The section directories in `dir` with their tasks, in the order of `state`.
fn sections(dir: &Path, state: &State) -> Result<Vec<Section>> {
    let mut section_dirs = visible_entries(dir, true)?;
    sort_by_order(&mut section_dirs, |dir| {
        state.section_position(file_name(dir))
    });

    let mut sections = Vec::new();
    for dir in section_dirs {
        let name = file_name(&dir).to_string();
        let task_paths = visible_entries(&dir, false)?
            .into_iter()
            .filter(|path| path.extension().is_some_and(|ext| ext == task::EXTENSION))
            .collect();
        sections.push(Section {
            tasks: load_tasks(task_paths, state, &name)?,
            gitkeep: read_if_exists(&dir.join(task::GITKEEP))?,
            name,
            dir,
        });
    }
    Ok(sections)
}

/// File content, `None` if the file does not exist.
pub fn read_if_exists(path: &Path) -> Result<Option<String>> {
    match fs::read_to_string(path) {
        Ok(content) => Ok(Some(content)),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(err).wrap_err_with(|| format!("failed to read {}", path.display())),
    }
}

/// Where a task of `section` done on `date` goes in the archive of `root`, with what is
/// there already: `done`, the archived tasks of that day.
pub fn archive_dir(
    root: &Path,
    date: Date,
    section: &str,
    done: &[archive::Done],
) -> archive::ArchiveDir {
    let path = root.join(archive::DIR).join(date.to_string()).join(section);
    let mut missing: Vec<PathBuf> = (path.ancestors())
        .take_while(|dir| !dir.exists())
        .map(Path::to_path_buf)
        .collect();
    missing.reverse();
    let taken = (done.iter())
        .filter(|done| done.section == section)
        .map(|done| done.task.stem().to_string())
        .collect();
    archive::ArchiveDir {
        path,
        missing,
        taken,
    }
}

/// The done tasks per day in `range` from the archive in `root`, each day read like the root
/// without a state file: sections and tasks by name. Entries whose name is no date are skipped.
/// No `.archive/` yet means nothing was done.
pub fn archive(root: &Path, range: impl RangeBounds<Date>) -> Result<archive::Archive> {
    let dir = root.join(archive::DIR);
    let mut days = archive::Archive::new();
    if !dir.is_dir() {
        return Ok(days);
    }
    for path in visible_entries(&dir, true)? {
        let Ok(date) = file_name(&path).parse::<Date>() else {
            continue;
        };
        if !range.contains(&date) {
            continue;
        }
        days.insert(date, day(&path)?);
    }
    Ok(days)
}

/// The done tasks of `date` from the archive in `root`, `None` without a directory for it.
pub fn archive_day(root: &Path, date: Date) -> Result<Option<Vec<archive::Done>>> {
    let path = root.join(archive::DIR).join(date.to_string());
    if !path.is_dir() {
        return Ok(None);
    }
    day(&path).map(Some)
}

/// A day directory of the archive, read like the root without a state file.
fn day(path: &Path) -> Result<Vec<archive::Done>> {
    let done = sections(path, &State::default())?
        .into_iter()
        .flat_map(|section| {
            section.tasks.into_iter().map(move |task| archive::Done {
                section: section.name.clone(),
                task,
            })
        })
        .collect();
    Ok(done)
}

fn load_tasks(mut paths: Vec<PathBuf>, state: &State, section: &str) -> Result<Vec<Task>> {
    sort_by_order(&mut paths, |path| {
        state.task_position(section, task::stem(path).unwrap_or_default())
    });
    paths
        .into_iter()
        .map(|path| {
            let content = fs::read_to_string(&path)
                .wrap_err_with(|| format!("failed to read {}", path.display()))?;
            let mut task = Task::parse(path, &content);
            task.last_done = state.last_done(section, task.stem());
            Ok(task)
        })
        .collect()
}

/// The directories in `dir`, or with `dirs` false the files, without hidden ones (`.git`,
/// `.markoff`, `.archive`, editor swap files).
fn visible_entries(dir: &Path, dirs: bool) -> Result<Vec<PathBuf>> {
    let read_dir =
        fs::read_dir(dir).wrap_err_with(|| format!("failed to read {}", dir.display()))?;

    let mut paths = Vec::new();
    for entry in read_dir {
        let entry = entry?;
        let path = entry.path();
        if file_name(&path).starts_with('.') {
            continue;
        }
        // The type mostly comes with the listing, a `stat` per entry is slow on some file
        // systems. Only a symlink needs one, to see what it points to.
        let file_type = entry.file_type()?;
        let wanted = match (file_type.is_symlink(), dirs) {
            (true, true) => path.is_dir(),
            (true, false) => path.is_file(),
            (false, true) => file_type.is_dir(),
            (false, false) => file_type.is_file(),
        };
        if wanted {
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
    fn archive_by_date_skips_other_entries() {
        let dir = fixture(&[
            (
                ".archive/2026-10-14/work/fix-bug.md",
                "# Fix bug\n\nnotes\n",
            ),
            (".archive/2026-10-14/work/b.md", ""),
            (".archive/2026-10-14/home/c.md", ""),
            (".archive/2026-10-02/", ""),
            (".archive/2026-10-03.md", "# Old format\n"),
            (".archive/notes/work/a.md", ""),
        ]);
        let days = archive(dir.path(), ..).unwrap();
        let some = archive(dir.path(), .."2026-10-13".parse::<Date>().unwrap()).unwrap();
        assert_eq!(
            some.keys().collect::<Vec<_>>(),
            [&"2026-10-02".parse().unwrap()]
        );
        let dates: Vec<String> = days.keys().map(Date::to_string).collect();
        assert_eq!(dates, ["2026-10-02", "2026-10-14"]);
        let done: Vec<String> = days[&"2026-10-14".parse().unwrap()]
            .iter()
            .map(|done| format!("{}/{}/{}", done.section, done.task.title(), done.task.body))
            .collect();
        assert_eq!(done, ["home/c/", "work/b/", "work/Fix bug/notes\n"]);
    }

    #[test]
    fn archive_day_reads_one_day() {
        let dir = fixture(&[
            (".archive/2026-10-14/work/a.md", "# A"),
            (".archive/2026-10-15/work/b.md", "# B"),
            (".archive/2026-10-16/", ""),
        ]);
        let day = |date: &str| archive_day(dir.path(), date.parse().unwrap()).unwrap();
        let titles: Vec<String> = (day("2026-10-14").unwrap().iter())
            .map(|done| done.task.title())
            .collect();
        assert_eq!(titles, ["A"]);
        assert_eq!(
            day("2026-10-16"),
            Some(Vec::new()),
            "a day without tasks is a day"
        );
        assert_eq!(day("2026-10-17"), None);
    }

    #[test]
    fn archive_without_dir_is_empty() {
        let dir = fixture(&[]);
        assert!(archive(dir.path(), ..).unwrap().is_empty());
    }

    fn titles(section: &Section) -> Vec<String> {
        section.tasks.iter().map(Task::title).collect()
    }

    fn section_names(sections: &[Section]) -> Vec<String> {
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

        assert_eq!(section_names(&sections), ["work", "private", "home"]);
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

        assert_eq!(section_names(&sections), ["work", "home", "misc"]);
        assert_eq!(titles(&sections[0]), ["b", "a", "c"]);
    }

    #[test]
    fn without_state_file_by_name() {
        // Old numbered names keep their order this way until they are renamed.
        let dir = fixture(&[("work/02-b.md", ""), ("work/01-a.md", ""), ("01-home/", "")]);
        let (sections, _) = load(dir.path()).unwrap();
        assert_eq!(section_names(&sections), ["01-home", "work"]);
        assert_eq!(titles(&sections[1]), ["01 a", "02 b"]);
    }

    #[test]
    fn ignores_root_files_hidden_non_markdown_and_nested() {
        let dir = fixture(&[
            ("readme.md", "# Notes"),
            (".git/config", ""),
            (".archive/2026-10-14/work/a.md", ""),
            ("work/.swap.md", ""),
            ("work/readme.txt", ""),
            ("work/nested/deep.md", ""),
            ("work/task.md", "# Task"),
        ]);
        let (sections, _) = load(dir.path()).unwrap();

        assert_eq!(section_names(&sections), ["work"]);
        assert_eq!(titles(&sections[0]), ["Task"]);
    }

    #[test]
    fn follows_symlinks_by_what_they_point_to() {
        use std::os::unix::fs::symlink;
        let dir = fixture(&[("work/task.md", "# Task"), ("outside/note.md", "# Note")]);
        let root = dir.path();
        symlink(root.join("outside"), root.join("linked")).unwrap();
        symlink(root.join("work/task.md"), root.join("work/link.md")).unwrap();
        // A link to a directory is no task.
        symlink(root.join("outside"), root.join("work/dir.md")).unwrap();
        let (sections, _) = load(root).unwrap();

        assert_eq!(section_names(&sections), ["linked", "outside", "work"]);
        assert_eq!(titles(&sections[0]), ["Note"]);
        assert_eq!(titles(&sections[2]), ["Task", "Task"]);
    }

    #[test]
    fn missing_dir_is_an_error() {
        let dir = fixture(&[]);
        let err = load(&dir.path().join("nope")).unwrap_err();
        assert!(err.to_string().contains("failed to read"));
    }
}
