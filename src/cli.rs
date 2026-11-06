//! The command line: options and commands for scripting, run instead of the TUI and printing
//! to stdout.
//!
//! The TUI stays the main way to use markoff, commands only make it scriptable. Parsed by clap,
//! which also writes `--help` from the doc comments of [`Cli`] and [`Command`]. Commands name
//! tasks and sections by their id, see [`id`].

mod id;
mod read;
mod write;

use std::path::{Path, PathBuf};

use clap::{Parser, Subcommand};
use color_eyre::Result;
use jiff::civil::Date;

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
    /// Print sections and tasks as markdown, in the order of the TUI, with each task's id
    List(read::ListArgs),
    /// Print a task as it is, also an archived one
    Show {
        /// Id of the task, as `list` prints it: `work/fix-the-bug`
        id: String,
    },
    /// Add a task at the end of an existing section, print its path
    Add {
        /// Name of the section
        section: String,
        /// Title of the task, the filename is made from it
        title: String,
    },
}

/// Runs `command` on the sections in `root` and returns what to print.
pub fn run(command: Command, root: &Path, today: Date) -> Result<String> {
    match command {
        Command::List(args) => read::list(root, &args, today),
        Command::Show { id } => read::show(root, &id),
        Command::Add { section, title } => write::add(root, &section, &title),
    }
}

#[cfg(test)]
pub mod tests {
    use std::fs;

    use clap::{CommandFactory, error::ErrorKind};
    use tempfile::TempDir;

    use super::*;

    fn parse(args: &[&str]) -> Result<Cli, clap::Error> {
        Cli::try_parse_from([&["markoff"], args].concat())
    }

    /// A root with section `work` holding `fix-the-bug` and `call-bob`, and an empty `home`.
    pub fn root() -> TempDir {
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

    #[test]
    fn parse_commands() {
        Cli::command().debug_assert();
        let command = |args: &[&str]| parse(args).unwrap().command;
        assert_eq!(command(&[]), None);
        assert_eq!(
            command(&["list"]),
            Some(Command::List(read::ListArgs::default()))
        );
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
        assert_eq!(error(&["add", "work"]), ErrorKind::MissingRequiredArgument);
        assert_eq!(
            error(&["add", "work", "Fix", "it"]),
            ErrorKind::UnknownArgument
        );
        assert_eq!(error(&["nope"]), ErrorKind::InvalidSubcommand);
        assert_eq!(error(&["--dir"]), ErrorKind::InvalidValue);
        assert_eq!(error(&["--nope"]), ErrorKind::UnknownArgument);
        assert_eq!(error(&["show"]), ErrorKind::MissingRequiredArgument);
        assert_eq!(
            error(&["list", "--hidden", "--archive"]),
            ErrorKind::ArgumentConflict
        );
        assert_eq!(
            error(&["list", "--from", "2026-11-01"]),
            ErrorKind::MissingRequiredArgument
        );
        assert_eq!(
            error(&["list", "--archive", "--to", "monday"]),
            ErrorKind::ValueValidation
        );
    }
}
