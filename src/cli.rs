//! Commands for scripting, run instead of the TUI and printing to stdout.
//!
//! The TUI stays the main way to use markoff, commands only make it scriptable. Options like
//! `--dir` are parsed in [`crate::config`], so a command only sees its own arguments.

use std::path::Path;

use color_eyre::{Result, eyre::bail};
use jiff::civil::Date;

use crate::{
    config::usage_error,
    edit, load, op,
    task::{HEADING, Section},
};

/// A command given as argument, without one markoff starts the TUI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// `list`: sections and tasks as markdown, see [`list`].
    List,
    /// `add <section> <title>`: a task at the end of the section, see [`add`].
    Add { section: String, title: String },
}

impl Command {
    /// The command from the args that are not options, `None` without any. Each command
    /// takes an exact number of args, so more can be added later without ambiguity.
    pub fn parse(args: &[String]) -> Result<Option<Self>> {
        let [name, rest @ ..] = args else {
            return Ok(None);
        };
        let command = match (name.as_str(), rest) {
            ("list", []) => Self::List,
            ("list", _) => return Err(usage_error("list takes no arguments")),
            ("add", [section, title]) => Self::Add {
                section: section.clone(),
                title: title.clone(),
            },
            ("add", _) => return Err(usage_error("add needs <section> <title>")),
            _ => return Err(usage_error(&format!("unknown command '{name}'"))),
        };
        Ok(Some(command))
    }
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

    use tempfile::TempDir;

    use super::*;

    fn parse(args: &[&str]) -> Result<Option<Command>> {
        Command::parse(&args.iter().map(|s| s.to_string()).collect::<Vec<_>>())
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
        assert_eq!(parse(&[]).unwrap(), None);
        assert_eq!(parse(&["list"]).unwrap(), Some(Command::List));
        assert_eq!(
            parse(&["add", "work", "Fix it"]).unwrap(),
            Some(Command::Add {
                section: "work".to_string(),
                title: "Fix it".to_string(),
            })
        );
        let error = |args: &[&str]| {
            let error = parse(args).unwrap_err().to_string();
            let (message, help) = error.split_once('\n').unwrap();
            assert_eq!(help, "Try 'markoff --help' for more information.");
            message.to_string()
        };
        assert_eq!(error(&["list", "work"]), "list takes no arguments");
        assert_eq!(error(&["add", "work"]), "add needs <section> <title>");
        assert_eq!(
            error(&["add", "work", "Fix", "it"]),
            "add needs <section> <title>"
        );
        assert_eq!(error(&["nope"]), "unknown command 'nope'");
    }

    #[test]
    fn list_sections_with_tasks() {
        let dir = root();
        assert_eq!(
            run(Command::List, dir.path()).unwrap(),
            "## work\n- Fix the bug\n- call bob\n\n## home\n"
        );
        assert_eq!(list(&[], jiff::Zoned::now().date()), "");

        // Repeating tasks only while due, like the TUI.
        let root = dir.path();
        fs::write(root.join("home/water.md"), "# Water `* * *`\n").unwrap();
        fs::write(root.join("home/dust.md"), "# Dust `* * *`\n").unwrap();
        let today = jiff::Zoned::now().date();
        let state = format!("# work\nfix-the-bug\ncall-bob\n\n# home\nwater\ndust {today}\n");
        fs::write(root.join(".markoff"), state).unwrap();
        assert_eq!(
            run(Command::List, root).unwrap(),
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
