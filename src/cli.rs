//! The command line: options and commands for scripting, run instead of the TUI and printing
//! to stdout.
//!
//! A command for every action of the TUI but undo, for scripts and agents. Parsed by clap,
//! which also writes `--help` from the doc comments of [`Cli`] and [`Command`]. Commands name
//! tasks and sections by their id, see [`id`].

mod id;
mod read;
mod write;

use std::path::{Path, PathBuf};

use clap::{Parser, Subcommand};
use color_eyre::Result;
use jiff::civil::Date;

use crate::config::Config;

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

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Print a guide for AI agents: what markoff is and how to use these commands
    Skill,
    /// Print sections and tasks as markdown, in the order of the TUI, with each task's id
    List(read::ListArgs),
    /// Print a task as it is, also an archived one
    Show {
        /// Id of the task, as `list` prints it: `work/fix-the-bug`
        id: String,
    },
    /// Add a task to a section, or a section with --section, print its id
    Add(write::AddArgs),
    /// Change the title or body of a task, or the name of a section, print its id
    Edit(write::EditArgs),
    /// Move a task within its section or into another one, or a section, print its id
    Move(write::MoveArgs),
    /// Delete tasks, or empty sections
    Delete {
        /// Ids of the tasks or sections
        #[arg(required = true)]
        ids: Vec<String>,
    },
    /// Archive tasks as done, a repeating one is hidden until it is due, print where each went
    Done {
        /// Ids of the tasks
        #[arg(required = true)]
        ids: Vec<String>,
    },
    /// Archive tasks as canceled, their heading struck through, like done for a repeating one
    Cancel {
        /// Ids of the tasks
        #[arg(required = true)]
        ids: Vec<String>,
    },
}

/// What `skill` prints, for agents. Short, it goes into their context: the details are in the
/// errors and in `--help`.
pub const SKILL: &str = include_str!("cli/skill.md");

/// Runs `command` on the sections in `root` and returns what to print.
pub fn run(command: Command, root: &Path, config: &Config, today: Date) -> Result<String> {
    match command {
        Command::Skill => unreachable!("run by main, it needs no root"),
        Command::List(args) => read::list(root, &args, today),
        Command::Show { id } => read::show(root, &id),
        Command::Add(mut args) => {
            args.body = stdin(args.body)?;
            write::add(root, &args, config)
        }
        Command::Edit(mut args) => {
            args.body = stdin(args.body)?;
            args.append = stdin(args.append)?;
            write::edit(root, &args, config)
        }
        Command::Move(args) => write::move_to(root, &args, config),
        Command::Delete { ids } => write::delete(root, &ids, config),
        Command::Done { ids } => write::done(root, &ids, false, today, config),
        Command::Cancel { ids } => write::done(root, &ids, true, today, config),
    }
}

/// The text of an option, read from stdin for `-`, for text too long or awkward to quote.
fn stdin(text: Option<String>) -> Result<Option<String>> {
    match text.as_deref() {
        Some("-") => Ok(Some(std::io::read_to_string(std::io::stdin())?)),
        _ => Ok(text),
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

    /// Runs markoff with `args` on `root`, as on the command line.
    pub fn markoff(root: &Path, args: &[&str]) -> Result<String> {
        let command = parse(args)?.command.expect("a command");
        run(
            command,
            root,
            &crate::config::defaults(),
            crate::app::tests::TODAY,
        )
    }

    #[test]
    fn parse_commands() {
        Cli::command().debug_assert();
        let command = |args: &[&str]| parse(args).unwrap().command;
        assert!(command(&[]).is_none());
        assert!(matches!(command(&["list"]), Some(Command::List(_))));
        let add = format!("{:?}", command(&["add", "work", "Fix it"]));
        assert!(add.starts_with("Some(Add("), "{add}");

        // Options anywhere, also between the arguments of a command.
        let dir = |args: &[&str]| parse(args).unwrap().dir;
        assert_eq!(dir(&["--dir", "/a", "list"]), Some(PathBuf::from("/a")));
        assert_eq!(dir(&["list", "--dir=/a"]), Some(PathBuf::from("/a")));
        assert_eq!(
            format!("{:?}", command(&["add", "work", "--dir", "/a", "Fix it"])),
            add
        );

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

    #[test]
    fn skill_has_every_command() {
        for command in Cli::command().get_subcommands() {
            let name = command.get_name();
            if !["help", "skill"].contains(&name) {
                assert!(
                    SKILL.contains(&format!("markoff {name} ")),
                    "{name} is missing"
                );
            }
        }
    }
}
