//! Commands that only read: `list` and `show`. No normalize, it would rename files behind an
//! editor.

use std::path::Path;

use clap::Args;
use color_eyre::{Result, eyre::bail};
use jiff::{ToSpan, civil::Date};

use super::id;
use crate::{load, task::Task};

#[derive(Debug, Clone, Default, PartialEq, Eq, Args)]
pub struct ListArgs {
    /// Only these sections
    #[arg(value_name = "SECTION")]
    sections: Vec<String>,
    /// Only tasks with QUERY in their title or body, ignoring case, like `/` in the TUI
    #[arg(long, value_name = "QUERY")]
    filter: Option<String>,
    /// Also repeating tasks that are not due, with the day they are due again
    #[arg(long, conflicts_with = "archive")]
    hidden: bool,
    /// Archived tasks by day instead, done and canceled (struck through)
    #[arg(long)]
    archive: bool,
    /// First day of the archive, default 6 days before --to
    #[arg(long, requires = "archive", value_name = "YYYY-MM-DD")]
    from: Option<Date>,
    /// Last day of the archive, default today
    #[arg(long, requires = "archive", value_name = "YYYY-MM-DD")]
    to: Option<Date>,
}

/// Sections as `## name` with their tasks as list items, in the order of the TUI. Like the TUI
/// without `.`, repeating tasks only while they are due, with a filter only sections with
/// matches. With `--archive` the archive instead, see [`list_archive`].
pub fn list(root: &Path, args: &ListArgs, today: Date) -> Result<String> {
    if args.archive {
        return list_archive(root, args, today);
    }
    let (sections, _) = load::load(root)?;
    for name in &args.sections {
        id::section(name, &id::names(&sections))?;
    }
    let query = args.filter.as_deref().unwrap_or_default();
    let text = sections
        .iter()
        .filter(|section| args.sections.is_empty() || args.sections.contains(&section.name))
        .filter_map(|section| {
            let tasks: Vec<String> = (section.tasks.iter())
                .filter(|task| (args.hidden || task.due(today)) && task.matches(query))
                .map(|task| {
                    let due = match (&task.repeat, task.last_done) {
                        (Some(repeat), Some(last_done)) if !task.due(today) => {
                            match repeat.next(last_done) {
                                Some(next) => format!(", due {next}"),
                                None => ", never due".to_string(),
                            }
                        }
                        _ => String::new(),
                    };
                    item(root, task, &due)
                })
                .collect();
            (query.is_empty() || !tasks.is_empty())
                .then(|| format!("## {}\n{}", section.name, tasks.concat()))
        })
        .collect::<Vec<_>>()
        .join("\n");
    Ok(text)
}

/// The archived tasks from `--from` to `--to` as `## day`, `### section`, then the tasks, by
/// name like the archive page. Days without tasks are left out.
fn list_archive(root: &Path, args: &ListArgs, today: Date) -> Result<String> {
    let to = args.to.unwrap_or(today);
    let from = args.from.unwrap_or(to.saturating_sub(6.days()));
    if from > to {
        bail!("--from {from} is after --to {to}");
    }
    let archive = load::archive(root)?;
    // A section of the archive may be gone from the root, so both count.
    let (sections, _) = load::load(root)?;
    let mut names = id::names(&sections);
    for done in archive.values().flatten() {
        if !names.contains(&done.section.as_str()) {
            names.push(&done.section);
        }
    }
    for name in &args.sections {
        id::section(name, &names)?;
    }
    let query = args.filter.as_deref().unwrap_or_default();
    let text = archive
        .range(from..=to)
        .filter_map(|(day, done)| {
            let mut text = format!("## {day}\n");
            let mut section = None;
            for done in done.iter().filter(|done| {
                (args.sections.is_empty() || args.sections.contains(&done.section))
                    && done.task.matches(query)
            }) {
                if section != Some(&done.section) {
                    text.push_str(&format!("### {}\n", done.section));
                    section = Some(&done.section);
                }
                text.push_str(&item(root, &done.task, ""));
            }
            section.is_some().then_some(text)
        })
        .collect::<Vec<_>>()
        .join("\n");
    Ok(text)
}

/// A task as list item: ``- Water plants `* * mon` (home/water-plants)``, the rule as in the
/// heading, a canceled one struck through like its heading. `extra` goes after the id.
fn item(root: &Path, task: &Task, extra: &str) -> String {
    let title = match task.canceled {
        true => format!("~~{}~~", task.title()),
        false => task.title(),
    };
    let rule = (task.repeat.as_ref())
        .map(|repeat| format!(" `{}`", repeat.rule))
        .unwrap_or_default();
    format!("- {title}{rule} ({}{extra})\n", id::of(root, task))
}

/// The content of the task with `id`, as it is.
pub fn show(root: &Path, id: &str) -> Result<String> {
    let (sections, _) = load::load(root)?;
    let archive = load::archive(root)?;
    let content = &id::task(id, root, &sections, &archive)?.content;
    // Ends the output in a newline, so the next prompt starts on a line of its own.
    match content.ends_with('\n') || content.is_empty() {
        true => Ok(content.clone()),
        false => Ok(format!("{content}\n")),
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use clap::Parser;

    use super::*;
    use crate::{
        app::tests::TODAY,
        cli::{Cli, Command, tests::root},
    };

    /// `list` with `args` as on the command line.
    fn list_with(root: &Path, args: &[&str]) -> Result<String> {
        let cli = Cli::try_parse_from([&["markoff", "list"], args].concat())?;
        let Some(Command::List(args)) = cli.command else {
            unreachable!()
        };
        list(root, &args, TODAY)
    }

    #[test]
    fn list_sections_with_tasks() {
        let dir = root();
        let root = dir.path();
        let list = |args: &[&str]| list_with(root, args).unwrap();
        assert_eq!(
            list(&[]),
            "## work\n- Fix the bug (work/fix-the-bug)\n- call bob (work/call-bob)\n\n## home\n"
        );
        assert_eq!(list(&["home"]), "## home\n");
        assert_eq!(
            list_with(root, &["wrok"]).unwrap_err().to_string(),
            "no section 'wrok', did you mean 'work'?"
        );

        // Repeating tasks only while due, like the TUI, all with --hidden.
        fs::write(root.join("home/water.md"), "# Water `* * *`\n").unwrap();
        fs::write(root.join("home/dust.md"), "# Dust `* * mon`\n").unwrap();
        fs::write(root.join("home/never.md"), "# Never `30 2 *`\n").unwrap();
        fs::write(
            root.join(".markoff"),
            format!(
                "# work\nfix-the-bug\ncall-bob\n\n# home\nwater\ndust {TODAY}\nnever {TODAY}\n"
            ),
        )
        .unwrap();
        assert_eq!(list(&["home"]), "## home\n- Water `* * *` (home/water)\n");
        assert_eq!(
            list(&["home", "--hidden"]),
            "## home\n- Water `* * *` (home/water)\n\
             - Dust `* * mon` (home/dust, due 2026-10-26)\n\
             - Never `30 2 *` (home/never, never due)\n"
        );

        // A filter in titles and bodies, sections without matches are left out.
        fs::write(root.join("home/water.md"), "# Water `* * *`\n\nthe BUG\n").unwrap();
        assert_eq!(
            list(&["--filter", "bug"]),
            "## work\n- Fix the bug (work/fix-the-bug)\n\n## home\n- Water `* * *` (home/water)\n"
        );
        assert_eq!(list(&["--filter", "nothing"]), "");

        // A canceled one by hand stays struck through.
        fs::write(root.join("work/call-bob.md"), "# ~~Call Bob~~\n").unwrap();
        assert_eq!(
            list(&["work"]),
            "## work\n- Fix the bug (work/fix-the-bug)\n- ~~Call Bob~~ (work/call-bob)\n"
        );
    }

    #[test]
    fn list_archive_by_day() {
        let dir = root();
        let root = dir.path();
        let archived = |day: &str, path: &str, content: &str| {
            let path = root.join(".archive").join(day).join(path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, content).unwrap();
        };
        // TODAY is 2026-10-22, the default is the 7 days up to it.
        archived("2026-10-15", "work/too-old.md", "# Too old\n");
        archived("2026-10-16", "work/b.md", "# B\n\nbug\n");
        archived("2026-10-16", "work/a.md", "# ~~A~~\n");
        archived("2026-10-16", "gone/c.md", "# C\n");
        archived("2026-10-22", "home/d.md", "# D\n");
        let list = |args: &[&str]| list_with(root, args).map_err(|err| err.to_string());

        assert_eq!(
            list(&["--archive"]).unwrap(),
            "## 2026-10-16\n### gone\n- C (.archive/2026-10-16/gone/c)\n\
             ### work\n- ~~A~~ (.archive/2026-10-16/work/a)\n- B (.archive/2026-10-16/work/b)\n\
             \n## 2026-10-22\n### home\n- D (.archive/2026-10-22/home/d)\n"
        );
        assert_eq!(
            list(&["--archive", "--from", "2026-10-01", "--to", "2026-10-15"]).unwrap(),
            "## 2026-10-15\n### work\n- Too old (.archive/2026-10-15/work/too-old)\n"
        );
        assert_eq!(
            list(&["--archive", "--to", "2026-10-18"]).unwrap(),
            list(&["--archive", "--from", "2026-10-12", "--to", "2026-10-18"]).unwrap()
        );
        // Sections and the filter like the open tasks, also a section gone from the root.
        assert_eq!(
            list(&["--archive", "gone"]).unwrap(),
            "## 2026-10-16\n### gone\n- C (.archive/2026-10-16/gone/c)\n"
        );
        assert_eq!(
            list(&["--archive", "--filter", "bug"]).unwrap(),
            "## 2026-10-16\n### work\n- B (.archive/2026-10-16/work/b)\n"
        );
        assert_eq!(
            list(&["--archive", "--from", "2026-10-23"]),
            Err("--from 2026-10-23 is after --to 2026-10-22".to_string())
        );
        assert_eq!(
            list(&["--archive", "shopping"]),
            Err("no section 'shopping', the sections: work, home, gone".to_string())
        );
    }

    #[test]
    fn show_prints_the_content() {
        let dir = root();
        let root = dir.path();
        assert_eq!(
            show(root, "work/fix-the-bug").unwrap(),
            "# Fix the bug\n\nbody\n"
        );
        fs::write(root.join("work/call-bob.md"), "no newline").unwrap();
        assert_eq!(show(root, "work/call-bob").unwrap(), "no newline\n");
        assert_eq!(
            show(root, "work/fix").unwrap_err().to_string(),
            "no task 'work/fix', did you mean 'work/fix-the-bug'?"
        );
    }
}
