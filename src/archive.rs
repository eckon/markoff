//! The archive in the root, laid out like the root itself, a directory per day:
//!
//! ```text
//! .archive/
//!   2026-10-14/
//!     work/
//!       fix-login-bug.md   <- the task file, moved here as it was
//! ```

use std::{collections::BTreeMap, path::PathBuf};

use jiff::civil::Date;

use crate::task::Task;

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
