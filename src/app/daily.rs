//! The daily checklist in the app, experimental like [`crate::daily`]: its rows on top of the
//! sections, the actions on them, and the path behind them. The actions work like on tasks,
//! each one an edit of the one file. Kept apart from the normal logic until
//! recurring tasks get a real design.

use std::{fs, path::PathBuf};

use color_eyre::{
    Result,
    eyre::{WrapErr, eyre},
};

use super::{App, Row, Status, today};
use crate::{daily, load, op::Op, task::split_heading};

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
        self.change_daily(|content| Some(daily::toggle(content, i, &today())))?;
        Ok(())
    }

    /// `D` on a daily task.
    pub(super) fn delete_daily(&mut self, i: usize) -> Result<()> {
        self.change_daily(|content| Some(daily::delete(content, i)))?;
        self.status = Some(Status::Info("task deleted (u to undo)".to_string()));
        Ok(())
    }

    /// `J` / `K` on a daily task: within the checklist, it stops at its edges.
    pub(super) fn move_daily(&mut self, i: usize, down: bool) -> Result<()> {
        if self.change_daily(|content| daily::move_task(content, i, down))? {
            let to = if down { i + 1 } else { i - 1 };
            self.select_path(&self.daily_row_path(Some(to)));
        }
        Ok(())
    }

    /// Inserts what was written in the editor for `o` / `O` at `index`.
    pub(super) fn after_create_daily(&mut self, index: usize, content: &str) -> Result<()> {
        let (Some(title), body) = split_heading(content) else {
            return Err(eyre!("aborted, empty title"));
        };
        self.change_daily(|daily| Some(daily::insert(daily, index, &title, body)))
            .map_err(|err| eyre!("task not added: {err}"))?;
        self.select_path(&self.daily_row_path(Some(index)));
        Ok(())
    }

    /// Writes `change` of the checklist file as one undo step. False if it changed nothing.
    fn change_daily(&mut self, change: impl FnOnce(&str) -> Option<String>) -> Result<bool> {
        let path = self.root.join(daily::FILE);
        let before = fs::read_to_string(&path)
            .wrap_err_with(|| format!("failed to read {}", path.display()))?;
        let Some(after) = change(&before) else {
            return Ok(false);
        };
        self.apply(vec![Op::Update {
            path,
            before,
            after,
        }])?;
        Ok(true)
    }
}

#[cfg(test)]
pub(in crate::app) mod tests {
    use std::fs;

    use tempfile::TempDir;

    use super::*;
    use crate::{
        app::{
            editor::EditorRequest,
            tests::{app_with_files, listing, type_filter},
        },
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
    fn daily_header_says_what_it_does_not_do() {
        let content = "## Jira\n";
        let (dir, mut app) = app_with_daily(&["work/a.md"], content);
        for action in [
            Action::MoveDown,
            Action::MoveUp,
            Action::Done,
            Action::Delete,
        ] {
            app.status = None;
            app.update(action);
            assert_eq!(
                app.status,
                Some(Status::Info("not on the daily header".to_string())),
                "{action:?}"
            );
        }
        assert_eq!(
            fs::read_to_string(dir.path().join("daily.md")).unwrap(),
            content
        );
        assert_eq!(listing(&app), ["[daily]", "Jira", "[work]", "a.md"]);

        // Like a section header: `<CR>` opens the file, `o` / `O` put a task first.
        app.update(Action::Open);
        let path = dir.path().join("daily.md");
        assert_eq!(app.editor_request, Some(EditorRequest::Open(path)));
        for action in [Action::NewBelow, Action::NewAbove] {
            app.update(action);
            assert_eq!(app.editor_request, Some(EditorRequest::CreateDaily(0)));
        }
    }

    #[test]
    fn daily_tasks_move_delete_and_open_like_tasks() {
        let content = "## Jira\n\n## Menu\n";
        let (dir, mut app) = app_with_daily(&["work/a.md"], content);
        let path = dir.path().join("daily.md");
        let read = || fs::read_to_string(&path).unwrap();
        app.cursor = 1;

        // `J` / `K` stay in the checklist, the cursor follows.
        app.update(Action::MoveUp);
        assert_eq!(read(), content, "at the top edge");
        app.update(Action::MoveDown);
        assert_eq!(read(), "## Menu\n\n## Jira\n");
        assert_eq!(app.selected(), Some(Row::DailyTask(1)));
        app.update(Action::MoveDown);
        assert_eq!(read(), "## Menu\n\n## Jira\n", "never into a section");

        app.update(Action::Delete);
        assert_eq!(read(), "## Menu\n");
        assert_eq!(
            app.status,
            Some(Status::Info("task deleted (u to undo)".to_string()))
        );
        app.update(Action::Undo);
        app.update(Action::Undo);
        assert_eq!(read(), content);
        assert_eq!(app.selected(), Some(Row::DailyTask(0)));

        app.update(Action::Open);
        assert_eq!(app.editor_request, Some(EditorRequest::Open(path.clone())));
        app.update(Action::NewBelow);
        assert_eq!(app.editor_request, Some(EditorRequest::CreateDaily(1)));
        app.update(Action::NewAbove);
        assert_eq!(app.editor_request, Some(EditorRequest::CreateDaily(0)));
    }

    #[test]
    fn daily_task_from_the_editor() {
        let (dir, mut app) = app_with_daily(&[], "## Jira\n\n## Menu\n");
        let path = dir.path().join("daily.md");

        app.after_create_daily(1, "# Standup\n\n10:00\n").unwrap();
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "## Jira\n\n## Standup\n\n10:00\n\n## Menu\n"
        );
        assert_eq!(app.selected(), Some(Row::DailyTask(1)));

        assert!(app.after_create_daily(0, "# \n").is_err(), "empty title");
        app.update(Action::Undo);
        assert_eq!(fs::read_to_string(&path).unwrap(), "## Jira\n\n## Menu\n");
    }
}
