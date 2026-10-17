//! File operations, the only way markoff changes the disk.
//!
//! Every [`Op`] has an exact [`Op::inverse`], which is what undo is built on.
//! A user action (insert, move, renumber, ...) is a list of ops, applied with [`apply_all`].

use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

use color_eyre::{
    Result,
    eyre::{WrapErr, bail, eyre},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Op {
    /// Writes a new file. Fails if it already exists.
    Create { path: PathBuf, content: String },
    /// Deletes a file. Keeps the content so the inverse can recreate it.
    /// Fails if the file no longer has that content, so changes made since are never lost.
    Remove { path: PathBuf, content: String },
    /// Renames a file or directory. Fails if the target already exists.
    Rename { from: PathBuf, to: PathBuf },
    /// Creates an empty directory. Fails if it already exists.
    CreateDir { path: PathBuf },
    /// Deletes a directory. Fails if it is not empty, so no file is ever lost with it.
    RemoveDir { path: PathBuf },
    /// Replaces the content of an existing file. Fails if it no longer has `before`.
    Update {
        path: PathBuf,
        before: String,
        after: String,
    },
}

impl Op {
    pub fn apply(&self) -> Result<()> {
        match self {
            Op::Create { path, content } => {
                ensure_free(path)?;
                write_atomic(path, content)
            }
            Op::Remove { path, content } => {
                ensure_content(path, content)?;
                fs::remove_file(path)
                    .wrap_err_with(|| format!("failed to remove {}", path.display()))
            }
            Op::Rename { from, to } => {
                ensure_free(to)?;
                fs::rename(from, to).wrap_err_with(|| {
                    format!("failed to rename {} to {}", from.display(), to.display())
                })
            }
            Op::Update {
                path,
                before,
                after,
            } => {
                ensure_content(path, before)?;
                write_atomic(path, after)
            }
            Op::CreateDir { path } => fs::create_dir(path)
                .wrap_err_with(|| format!("failed to create {}", path.display())),
            Op::RemoveDir { path } => fs::remove_dir(path)
                .wrap_err_with(|| format!("failed to remove {}", path.display())),
        }
    }

    /// The op that undoes this one.
    pub fn inverse(&self) -> Op {
        match self.clone() {
            Op::Create { path, content } => Op::Remove { path, content },
            Op::Remove { path, content } => Op::Create { path, content },
            Op::Rename { from, to } => Op::Rename { from: to, to: from },
            Op::Update {
                path,
                before,
                after,
            } => Op::Update {
                path,
                before: after,
                after: before,
            },
            Op::CreateDir { path } => Op::RemoveDir { path },
            Op::RemoveDir { path } => Op::CreateDir { path },
        }
    }
}

/// Applies all ops in order. If one fails, the ones already applied are rolled back,
/// so a multi-file action (e.g. renumbering a section) never stays half done.
pub fn apply_all(ops: &[Op]) -> Result<()> {
    for (i, op) in ops.iter().enumerate() {
        if let Err(err) = op.apply() {
            for done in ops[..i].iter().rev() {
                // Best effort: report the original error, a failed rollback is attached to it.
                if let Err(rollback_err) = done.inverse().apply() {
                    return Err(err.wrap_err(eyre!("rollback failed too: {rollback_err}")));
                }
            }
            return Err(err);
        }
    }
    Ok(())
}

/// Ops that undo `ops`: each one inverted, in reverse order.
pub fn inverse_all(ops: &[Op]) -> Vec<Op> {
    ops.iter().rev().map(Op::inverse).collect()
}

/// Refuses to touch a file that changed since it was read, so those changes are never lost.
fn ensure_content(path: &Path, expected: &str) -> Result<()> {
    let current =
        fs::read_to_string(path).wrap_err_with(|| format!("failed to read {}", path.display()))?;
    if current != expected {
        bail!("{} changed on disk, reload first", path.display());
    }
    Ok(())
}

/// `fs::rename` silently replaces an existing file on Unix, so check first.
///
/// There is a small race if another process creates the file in between. Acceptable for
/// a single-user todo list.
fn ensure_free(path: &Path) -> Result<()> {
    if path.exists() {
        bail!("{} already exists", path.display());
    }
    Ok(())
}

/// Writes to a hidden temp file in the same directory, then renames it over the target.
///
/// A crash leaves either the old file or the new one, never a half written file.
/// Same directory, because a rename across file systems is not atomic.
fn write_atomic(path: &Path, content: &str) -> Result<()> {
    let dir = path.parent().unwrap_or(Path::new("."));
    let mut tmp = tempfile::Builder::new()
        .prefix(".markoff-")
        .tempfile_in(dir)
        .wrap_err_with(|| format!("failed to create temp file in {}", dir.display()))?;

    tmp.write_all(content.as_bytes())?;
    tmp.as_file().sync_all()?;
    tmp.persist(path)
        .wrap_err_with(|| format!("failed to write {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn create(path: PathBuf, content: &str) -> Op {
        Op::Create {
            path,
            content: content.to_string(),
        }
    }

    /// Sorted file names in `dir`, including hidden ones (to catch leftover temp files).
    fn files(dir: &Path) -> Vec<String> {
        let mut names: Vec<_> = fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn write_atomic_replaces_content_without_leftovers() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("a.md");
        write_atomic(&path, "one").unwrap();
        write_atomic(&path, "two").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "two");
        assert_eq!(files(dir.path()), ["a.md"]);
    }

    #[test]
    fn create_and_inverse() {
        let dir = TempDir::new().unwrap();
        let op = create(dir.path().join("a.md"), "# A");

        op.apply().unwrap();
        assert_eq!(fs::read_to_string(dir.path().join("a.md")).unwrap(), "# A");

        op.inverse().apply().unwrap();
        assert!(files(dir.path()).is_empty());
    }

    #[test]
    fn remove_dir_refuses_non_empty_dir_and_inverse_creates_it() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("work");
        let op = Op::RemoveDir { path: path.clone() };
        fs::create_dir(&path).unwrap();
        fs::write(path.join(".hidden"), "").unwrap();

        // Hidden files don't show in markoff, but still keep the directory.
        assert!(op.apply().is_err());
        fs::remove_file(path.join(".hidden")).unwrap();

        op.apply().unwrap();
        assert!(files(dir.path()).is_empty());
        op.inverse().apply().unwrap();
        assert_eq!(files(dir.path()), ["work"]);
    }

    #[test]
    fn remove_refuses_changed_file() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("a.md");
        let op = create(path.clone(), "# A");
        op.apply().unwrap();

        // Edited in the editor after it was created.
        fs::write(&path, "# A\n\nnew notes").unwrap();

        assert!(op.inverse().apply().is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "# A\n\nnew notes");
    }

    #[test]
    fn update_and_inverse() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("a.md");
        create(path.clone(), "old").apply().unwrap();

        let op = Op::Update {
            path: path.clone(),
            before: "old".to_string(),
            after: "new".to_string(),
        };
        op.apply().unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "new");

        op.inverse().apply().unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "old");

        // Content is "old" again, so the inverse no longer matches.
        assert!(op.inverse().apply().is_err());
    }

    #[test]
    fn rename_and_inverse() {
        let dir = TempDir::new().unwrap();
        let (a, b) = (dir.path().join("a.md"), dir.path().join("b.md"));
        create(a.clone(), "x").apply().unwrap();

        let op = Op::Rename { from: a, to: b };
        op.apply().unwrap();
        assert_eq!(files(dir.path()), ["b.md"]);

        op.inverse().apply().unwrap();
        assert_eq!(files(dir.path()), ["a.md"]);
    }

    #[test]
    fn never_overwrites() {
        let dir = TempDir::new().unwrap();
        let (a, b) = (dir.path().join("a.md"), dir.path().join("b.md"));
        create(a.clone(), "a").apply().unwrap();
        create(b.clone(), "b").apply().unwrap();

        assert!(create(a.clone(), "new").apply().is_err());
        assert!(
            Op::Rename {
                from: a.clone(),
                to: b.clone()
            }
            .apply()
            .is_err()
        );
        assert_eq!(fs::read_to_string(&a).unwrap(), "a");
        assert_eq!(fs::read_to_string(&b).unwrap(), "b");
    }

    #[test]
    fn apply_all_rolls_back_on_failure() {
        let dir = TempDir::new().unwrap();
        let (a, b, c) = (
            dir.path().join("a.md"),
            dir.path().join("b.md"),
            dir.path().join("c.md"),
        );
        create(c.clone(), "existing").apply().unwrap();

        let ops = [
            create(a.clone(), "a"),
            Op::Rename { from: a, to: b },
            create(c, "clash"), // fails, c exists
        ];
        assert!(apply_all(&ops).is_err());
        assert_eq!(files(dir.path()), ["c.md"]);
    }

    #[test]
    fn inverse_all_undoes_in_reverse_order() {
        let dir = TempDir::new().unwrap();
        let (a, b) = (dir.path().join("a.md"), dir.path().join("b.md"));
        let ops = [create(a.clone(), "a"), Op::Rename { from: a, to: b }];

        apply_all(&ops).unwrap();
        assert_eq!(files(dir.path()), ["b.md"]);

        apply_all(&inverse_all(&ops)).unwrap();
        assert!(files(dir.path()).is_empty());
    }
}
