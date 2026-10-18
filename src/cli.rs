//! Commands for scripting, printed to stdout instead of starting the TUI. Read-only, so they
//! never normalize: that would rename files behind an editor, like on watcher reloads.

use std::path::Path;

use color_eyre::Result;

use crate::{load, task::Section};

/// A command given as argument, without one markoff starts the TUI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    /// `list`: sections and tasks as markdown, see [`list`].
    List,
}

/// The output of `command` on the sections in `root`.
pub fn run(command: Command, root: &Path) -> Result<String> {
    let (sections, _) = load::load(root)?;
    Ok(match command {
        Command::List => list(&sections),
    })
}

/// Sections as `## name` with their task titles as list items, in the order of the TUI.
pub fn list(sections: &[Section]) -> String {
    sections
        .iter()
        .map(|section| {
            let mut text = format!("## {}\n", section.name);
            for task in &section.tasks {
                text.push_str(&format!("- {}\n", task.title()));
            }
            text
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use super::*;

    #[test]
    fn list_sections_with_tasks() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join("work")).unwrap();
        fs::create_dir_all(root.join("empty")).unwrap();
        fs::write(root.join("work/fix-bug.md"), "# Fix the bug\n\nbody\n").unwrap();
        fs::write(root.join("work/call-bob.md"), "no heading\n").unwrap();
        fs::write(
            root.join(".order"),
            "# work\nfix-bug\ncall-bob\n\n# empty\n",
        )
        .unwrap();

        assert_eq!(
            run(Command::List, root).unwrap(),
            "## work\n- Fix the bug\n- call bob\n\n## empty\n"
        );
        assert_eq!(list(&[]), "");
    }
}
