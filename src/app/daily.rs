//! The daily checklist in the app, experimental like [`crate::daily`]: its rows on top of the
//! sections, `X` on them, and the path behind them. Kept apart from the normal logic until
//! recurring tasks get a real design.

use std::{fs, path::PathBuf};

use color_eyre::{Result, eyre::WrapErr};

use super::{App, Row, today};
use crate::{daily, load, op::Op};

impl App {
    /// Reads the checklist file, `None` if it does not exist.
    pub(super) fn reload_daily(&mut self) -> Result<()> {
        let path = self.root.join(daily::FILE);
        self.daily = load::read_if_exists(&path)?.map(|content| daily::parse(&content));
        Ok(())
    }

    /// The `daily` header and its tasks, on top of the sections. A filter hides them, the
    /// filter is for the sections.
    pub(super) fn daily_rows(&self) -> Vec<Row> {
        let mut rows = Vec::new();
        if self.filter.is_empty()
            && let Some(daily) = &self.daily
        {
            rows.push(Row::Daily);
            rows.extend((0..daily.len()).map(Row::DailyTask));
        }
        rows
    }

    /// The path behind the header, or task `i`: they share one file, so a task gets
    /// `daily.md#<i>`.
    pub(super) fn daily_row_path(&self, task: Option<usize>) -> PathBuf {
        let path = self.root.join(daily::FILE);
        match task {
            Some(i) => PathBuf::from(format!("{}#{i}", path.display())),
            None => path,
        }
    }

    /// `X` on a daily task: done for today, or open again. The task stays in the list.
    pub(super) fn toggle_daily(&mut self, i: usize) -> Result<()> {
        let path = self.root.join(daily::FILE);
        let before = fs::read_to_string(&path)
            .wrap_err_with(|| format!("failed to read {}", path.display()))?;
        let after = daily::toggle(&before, i, &today());
        self.apply(vec![Op::Update {
            path,
            before,
            after,
        }])
    }
}

#[cfg(test)]
pub(in crate::app) mod tests {
    use std::fs;

    use tempfile::TempDir;

    use super::*;
    use crate::{
        app::tests::{app_with_files, listing, type_filter},
        keymap::Action,
    };

    /// `app_with_files` plus a `daily.md` with `content`.
    pub(in crate::app) fn app_with_daily(files: &[&str], content: &str) -> (TempDir, App) {
        let (dir, mut app) = app_with_files(files);
        fs::write(dir.path().join(daily::FILE), content).unwrap();
        app.reload(None).unwrap();
        (dir, app)
    }

    #[test]
    fn daily_rows_first_hidden_by_filter() {
        let (_dir, mut app) = app_with_daily(&["work/a.md", "home/b.md"], "## Jira\n## Menu\n");
        assert_eq!(
            app.rows(),
            [
                Row::Daily,
                Row::DailyTask(0),
                Row::DailyTask(1),
                Row::Section(0),
                Row::Task(0, 0),
                Row::Section(1),
                Row::Task(1, 0),
            ]
        );
        app.cursor = 4;
        app.update(Action::PrevSection);
        app.update(Action::PrevSection);
        assert_eq!(
            app.selected(),
            Some(Row::Daily),
            "`{{` stops at the daily header"
        );

        // Even when a daily task would match.
        type_filter(&mut app, "jira");
        assert!(!app.rows().contains(&Row::Daily));
    }

    #[test]
    fn daily_done_toggles_and_undo_restores() {
        let content = "# Daily\n\n## Jira\n## Menu\n";
        let (dir, mut app) = app_with_daily(&[], content);
        let path = dir.path().join("daily.md");
        app.cursor = 2;

        app.update(Action::Done);
        let today = today();
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            format!("# Daily\n\n## Jira\n## Menu [done {today}]\n")
        );
        assert_eq!(app.selected(), Some(Row::DailyTask(1)), "stays in the list");
        assert!(app.daily.as_ref().unwrap()[1].done_on(&today));

        app.update(Action::Done);
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            content,
            "X again opens it"
        );

        app.update(Action::Undo);
        app.cursor = 0;
        app.update(Action::Undo);
        assert_eq!(fs::read_to_string(&path).unwrap(), content);
        assert_eq!(
            app.selected(),
            Some(Row::DailyTask(1)),
            "undo goes back to the task"
        );
    }

    #[test]
    fn daily_rows_ignore_other_keys() {
        let content = "## Jira\n";
        let (dir, mut app) = app_with_daily(&["work/a.md"], content);
        for cursor in [0, 1] {
            app.cursor = cursor;
            for action in [
                Action::MoveDown,
                Action::MoveUp,
                Action::Delete,
                Action::Open,
                Action::NewBelow,
                Action::NewAbove,
            ] {
                app.update(action);
                assert_eq!(app.status, None, "{action:?}");
                assert_eq!(app.editor_request, None, "{action:?}");
            }
        }
        assert_eq!(
            fs::read_to_string(dir.path().join("daily.md")).unwrap(),
            content
        );
        assert_eq!(listing(&app), ["[daily]", "Jira", "[work]", "a.md"]);
    }
}
