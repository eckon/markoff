//! Commands that change files. Each normalizes first, like every write in the TUI.

use std::path::Path;

use color_eyre::{Result, eyre::bail};

use super::id;
use crate::{edit, load, op, task::HEADING};

/// Writes a task with only the heading at the end of `section`, like `o` on its last task.
/// Returns the new path. Sections are a TUI decision, so a missing one is an error.
pub fn add(root: &Path, section: &str, title: &str) -> Result<String> {
    let title = title.trim();
    if title.is_empty() {
        bail!("cannot add: empty title");
    }
    let (sections, file) = load::load(root)?;
    let s = id::section(section, &id::names(&sections))?;
    // Normalize before writing, like every write in the TUI. It keeps the order of sections,
    // so `s` stays valid.
    op::apply_all(&edit::normalize(&sections, &file))?;
    let (sections, file) = load::load(root)?;
    let end = sections[s].tasks.len();
    let content = format!("{HEADING}{title}\n");
    let (ops, path) = edit::insert(&sections, s, end, title, &content, &file);
    op::apply_all(&ops)?;
    Ok(format!("{}\n", path.display()))
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use super::*;
    use crate::{
        app::tests::TODAY,
        cli::{Command, run, tests::root},
    };

    fn list(root: &Path) -> String {
        run(Command::List(Default::default()), root, TODAY).unwrap()
    }

    #[test]
    fn add_at_the_end_of_the_section() {
        let dir = root();
        let root = dir.path();
        let path = root.join("work/new-thing.md");
        assert_eq!(
            add(root, "work", " New thing ").unwrap(),
            format!("{}\n", path.display())
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), "# New thing\n");
        assert_eq!(
            fs::read_to_string(root.join(".markoff")).unwrap(),
            "# work\nfix-the-bug\ncall-bob\nnew-thing\n\n# home\n"
        );

        // A taken name gets a suffix, an empty section works too.
        add(root, "work", "New thing").unwrap();
        add(root, "home", "Water plants").unwrap();
        assert_eq!(
            list(root),
            "## work\n- Fix the bug (work/fix-the-bug)\n- call bob (work/call-bob)\n\
             - New thing (work/new-thing)\n- New thing (work/new-thing-2)\n\n\
             ## home\n- Water plants (home/water-plants)\n"
        );
        assert!(root.join("work/new-thing-2.md").exists());
    }

    #[test]
    fn add_normalizes_first() {
        let dir = root();
        let root = dir.path();
        fs::write(root.join("work/fix-the-bug.md"), "# Renamed\n").unwrap();
        add(root, "work", "New").unwrap();
        assert!(root.join("work/renamed.md").exists());
        assert_eq!(
            fs::read_to_string(root.join(".markoff")).unwrap(),
            "# work\nrenamed\ncall-bob\nnew\n\n# home\n"
        );
    }

    #[test]
    fn add_errors_change_nothing() {
        let dir = root();
        let root = dir.path();
        // Needs normalizing, so a write would show as a rename.
        fs::write(root.join("work/fix-the-bug.md"), "# Renamed\n").unwrap();
        let error = |section, title| add(root, section, title).unwrap_err().to_string();
        assert_eq!(
            error("wrok", "New"),
            "no section 'wrok', did you mean 'work'?"
        );
        assert_eq!(
            error("errands", "New"),
            "no section 'errands', the sections: work, home"
        );
        assert_eq!(error("work", "  "), "cannot add: empty title");
        assert!(root.join("work/fix-the-bug.md").exists());

        let empty = TempDir::new().unwrap();
        assert_eq!(
            add(empty.path(), "work", "New").unwrap_err().to_string(),
            "no section 'work', there are none yet"
        );
    }
}
