//! The command line: options and commands for scripting, run instead of the TUI and printing
//! to stdout.
//!
//! The TUI stays the main way to use markoff, commands only make it scriptable. Parsed by clap,
//! which also writes `--help` from the doc comments of [`Cli`] and [`Command`].

use std::path::{Path, PathBuf};

use clap::{Parser, Subcommand};
use color_eyre::{Result, eyre::bail};
use jiff::civil::Date;

use crate::{
    edit, load, op,
    task::{HEADING, Section},
};

/// A TUI for tasks stored as markdown files.
///
/// Without a command the TUI starts, commands make it scriptable.
#[derive(Debug, Parser)]
#[command(version)]
pub struct Cli {
    /// Root directory, else $MARKOFF_DIR, else `general.dir` in ~/.config/markoff/config.toml
    #[arg(long, global = true, value_name = "PATH")]
    pub dir: Option<PathBuf>,

    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Clone, PartialEq, Eq, Subcommand)]
pub enum Command {
    /// Print sections and tasks as markdown, in the order of the TUI
    List,
    /// Add a task at the end of an existing section, print its path
    Add {
        /// Name of the section
        section: String,
        /// Title of the task, the filename is made from it
        title: String,
    },
}

/// Runs `command` on the sections in `root` and returns what to print.
pub fn run(command: Command, root: &Path) -> Result<String> {
    match command {
        Command::List => Ok(list(&load::load(root)?.0, jiff::Zoned::now().date())),
        Command::Add { section, title } => add(root, &section, &title),
    }
}

/// Sections as `## name` with their task titles as list items, in the order of the TUI.
/// Like the TUI without `.`, repeating tasks only while they are due. Read-only, so no
/// normalize: that would rename files behind an editor.
pub fn list(sections: &[Section], today: Date) -> String {
    sections
        .iter()
        .map(|section| {
            let mut text = format!("## {}\n", section.name);
            for task in section.tasks.iter().filter(|task| task.due(today)) {
                text.push_str(&format!("- {}\n", task.title()));
            }
            text
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Writes a task with only the heading at the end of `section`, like `o` on its last task.
/// Returns the new path. Sections are a TUI decision, so a missing one is an error.
fn add(root: &Path, section: &str, title: &str) -> Result<String> {
    let title = title.trim();
    if title.is_empty() {
        bail!("cannot add: empty title");
    }
    let (sections, file) = load::load(root)?;
    let Some(s) = sections.iter().position(|sec| sec.name == section) else {
        bail!("cannot add to '{section}': no such section");
    };
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

    use clap::{CommandFactory, error::ErrorKind};
    use tempfile::TempDir;

    use super::*;
    use crate::app::tests::TODAY;

    fn parse(args: &[&str]) -> Result<Cli, clap::Error> {
        Cli::try_parse_from([&["markoff"], args].concat())
    }

    /// A root with section `work` holding `fix-the-bug` and `call-bob`, and an empty `home`.
    fn root() -> TempDir {
        let dir = TempDir::new().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join("work")).unwrap();
        fs::create_dir_all(root.join("home")).unwrap();
        fs::write(root.join("work/fix-the-bug.md"), "# Fix the bug\n\nbody\n").unwrap();
        fs::write(root.join("work/call-bob.md"), "no heading\n").unwrap();
        fs::write(
            root.join(".markoff"),
            "# work\nfix-the-bug\ncall-bob\n\n# home\n",
        )
        .unwrap();
        dir
    }

    fn add(root: &Path, section: &str, title: &str) -> Result<String> {
        let command = Command::Add {
            section: section.to_string(),
            title: title.to_string(),
        };
        run(command, root)
    }

    #[test]
    fn parse_commands() {
        Cli::command().debug_assert();
        let command = |args: &[&str]| parse(args).unwrap().command;
        assert_eq!(command(&[]), None);
        assert_eq!(command(&["list"]), Some(Command::List));
        let add = Some(Command::Add {
            section: "work".to_string(),
            title: "Fix it".to_string(),
        });
        assert_eq!(command(&["add", "work", "Fix it"]), add);

        // Options anywhere, also between the arguments of a command.
        let dir = |args: &[&str]| parse(args).unwrap().dir;
        assert_eq!(dir(&["--dir", "/a", "list"]), Some(PathBuf::from("/a")));
        assert_eq!(dir(&["list", "--dir=/a"]), Some(PathBuf::from("/a")));
        assert_eq!(command(&["add", "work", "--dir", "/a", "Fix it"]), add);

        let error = |args: &[&str]| parse(args).unwrap_err().kind();
        assert_eq!(error(&["list", "work"]), ErrorKind::UnknownArgument);
        assert_eq!(error(&["add", "work"]), ErrorKind::MissingRequiredArgument);
        assert_eq!(
            error(&["add", "work", "Fix", "it"]),
            ErrorKind::UnknownArgument
        );
        assert_eq!(error(&["nope"]), ErrorKind::InvalidSubcommand);
        assert_eq!(error(&["--dir"]), ErrorKind::InvalidValue);
        assert_eq!(error(&["--nope"]), ErrorKind::UnknownArgument);
    }

    #[test]
    fn list_sections_with_tasks() {
        let dir = root();
        assert_eq!(
            run(Command::List, dir.path()).unwrap(),
            "## work\n- Fix the bug\n- call bob\n\n## home\n"
        );
        assert_eq!(list(&[], TODAY), "");

        // Repeating tasks only while due, like the TUI.
        let root = dir.path();
        fs::write(root.join("home/water.md"), "# Water `* * *`\n").unwrap();
        fs::write(root.join("home/dust.md"), "# Dust `* * *`\n").unwrap();
        fs::write(
            root.join(".markoff"),
            format!("# work\nfix-the-bug\ncall-bob\n\n# home\nwater\ndust {TODAY}\n"),
        )
        .unwrap();
        assert_eq!(
            list(&load::load(root).unwrap().0, TODAY),
            "## work\n- Fix the bug\n- call bob\n\n## home\n- Water\n"
        );
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
            run(Command::List, root).unwrap(),
            "## work\n- Fix the bug\n- call bob\n- New thing\n- New thing\n\n\
             ## home\n- Water plants\n"
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
            "cannot add to 'wrok': no such section"
        );
        assert_eq!(error("work", "  "), "cannot add: empty title");
        assert!(root.join("work/fix-the-bug.md").exists());
    }
}
