//! The archive in the root, laid out like the root itself, a directory per day:
//!
//! ```text
//! .archive/
//!   2026-10-14/
//!     work/
//!       fix-login-bug.md   <- the task file, moved here as it was
//! ```

use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Component, Path, PathBuf},
};

use jiff::civil::Date;

use crate::task::{Section, Task};

/// Directory in the root. Hidden, so it is never loaded as a section.
pub const DIR: &str = ".archive";

/// The archived tasks per day, done or canceled, only days with an archive directory, see
/// [`crate::load::archive`].
pub type Archive = BTreeMap<Date, Vec<Done>>;

/// The directory a done task goes to, `.archive/<day>/<section>`, as it is on disk, see
/// [`crate::load::archive_dir`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchiveDir {
    pub path: PathBuf,
    /// The directories up to `path` that do not exist yet, outermost first: created as ops,
    /// so undo leaves no empty ones behind.
    pub missing: Vec<PathBuf>,
    /// The names already in it, a task done twice a day gets a number.
    pub taken: Vec<String>,
}

/// A task in the archive with the name of its section.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Done {
    pub section: String,
    pub task: Task,
}

/// The section names of `archive`: those of the root's `sections` in their order, so they keep
/// their key and color, then the ones only in the archive by name.
pub fn sections<'a>(sections: &'a [Section], archive: &'a Archive) -> Vec<&'a str> {
    let mut names: Vec<&str> = sections.iter().map(|s| s.name.as_str()).collect();
    let gone: BTreeSet<&str> = (archive.values().flatten())
        .map(|done| done.section.as_str())
        .filter(|name| !names.contains(name))
        .collect();
    names.extend(gone);
    names
}

/// The days of the archive that changes at `paths`, relative to the root, touch. `None` when one
/// is the archive as a whole: `.archive` itself, or the root above it. Paths outside the archive
/// touch no day.
pub fn days_touched<'a>(paths: impl IntoIterator<Item = &'a Path>) -> Option<BTreeSet<Date>> {
    let mut days = BTreeSet::new();
    for path in paths {
        let mut parts = path.components().filter(|part| *part != Component::CurDir);
        match (parts.next(), parts.next()) {
            (None, _) => return None,
            (Some(first), day) if first.as_os_str() == DIR => {
                let day = day?.as_os_str().to_str();
                days.extend(day.and_then(|day| day.parse::<Date>().ok()));
            }
            _ => {}
        }
    }
    Some(days)
}

#[cfg(test)]
pub mod tests {
    use super::*;

    /// A task archived in `section`, read from `content`. Not on disk.
    pub fn done(section: &str, content: &str) -> Done {
        Done {
            section: section.to_string(),
            task: Task::parse(PathBuf::new(), content),
        }
    }

    #[test]
    fn days_touched_by_paths() {
        let touched = |paths: &[&str]| days_touched(paths.iter().map(Path::new));
        let day = |date: &str| date.parse::<Date>().unwrap();
        assert_eq!(
            touched(&[
                ".archive/2026-10-14/work/a.md",
                ".archive/2026-10-15",
                "work/b.md",
                ".archive/notes/c.md",
            ]),
            Some([day("2026-10-14"), day("2026-10-15")].into())
        );
        assert_eq!(touched(&["work/b.md", ".markoff"]), Some(BTreeSet::new()));
        assert_eq!(touched(&[".archive"]), None);
        assert_eq!(touched(&[""]), None, "the root");
    }
}
