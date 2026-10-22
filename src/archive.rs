//! The archive in the root, laid out like the root itself, a directory per day:
//!
//! ```text
//! .archive/
//!   2026-10-14/
//!     work/
//!       fix-login-bug.md   <- the task file, moved here as it was
//! ```

use crate::task::Task;

/// Directory in the root. Hidden, so it is never loaded as a section.
pub const DIR: &str = ".archive";

/// A task in the archive with the name of its section.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Done {
    pub section: String,
    pub task: Task,
}
