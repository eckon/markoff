//! The changes of the `change` group that need no editor: done and cancel, move, delete and
//! undo. Each one is a list of ops from [`edit`], applied as one undo step.

use color_eyre::{Result, eyre::eyre};

use super::{App, Row, Status};
use crate::{edit, load, op};

impl App {
    /// `x`, and `X` with `cancel`. A repeating task is the same either way, it is only there
    /// to come back.
    pub(super) fn done(&mut self, cancel: bool) -> Result<()> {
        let (s, t) = match self.selected() {
            Some(Row::Task(s, t)) => (s, t),
            _ => return Ok(()),
        };
        let date = self.today;
        if self.sections[s].tasks[t].repeat.is_some() {
            let ops = edit::repeat_done(&self.sections, s, t, date, &self.state_file);
            if self.apply(ops)? {
                self.status = Some(Status::Info("done until next time (u to undo)".to_string()));
            }
            return Ok(());
        }
        let to = load::archive_dir(&self.root, date, &self.sections[s].name, &self.archive);
        let (ops, _) = edit::archive(&self.sections, s, t, cancel, &to, &self.state_file);
        self.apply(ops)?;
        let message = match cancel {
            true => "task canceled (u to undo)",
            false => "task archived (u to undo)",
        };
        self.status = Some(Status::Info(message.to_string()));
        Ok(())
    }

    /// `J` / `K`: moves the task or section under the cursor one place, the cursor follows.
    /// Nothing happens at the edges.
    pub(super) fn move_selected(&mut self, down: bool) -> Result<()> {
        let file = &self.state_file;
        let (ops, moved_to) = match self.selected() {
            Some(Row::Task(s, t)) => {
                let Some((to, index)) = self.task_target(s, t, down) else {
                    return Ok(());
                };
                edit::move_task(&self.sections, s, t, to, index, file)
            }
            Some(Row::Section(s)) => {
                let index = match down {
                    true => s + 1,
                    false => s.wrapping_sub(1),
                };
                if index >= self.sections.len() {
                    return Ok(());
                }
                edit::move_section(&self.sections, s, index, file)
            }
            None => return Ok(()),
        };
        self.apply(ops)?;
        self.select_path(&moved_to);
        Ok(())
    }

    /// Where `J` / `K` put task `t` of section `s`, as section and index among the other
    /// tasks there. At a section edge it goes into the neighbouring section, first or last.
    /// `None` at the very top or bottom.
    ///
    /// Hidden tasks are stepped over, so the task always passes one that can be seen.
    fn task_target(&self, s: usize, t: usize, down: bool) -> Option<(usize, usize)> {
        let tasks = &self.sections[s].tasks;
        let shown = |&i: &usize| self.shown(&tasks[i]);
        if down {
            match (t + 1..tasks.len()).find(shown) {
                Some(next) => Some((s, next)),
                None => (s + 1 < self.sections.len()).then_some((s + 1, 0)),
            }
        } else {
            match (0..t).rev().find(shown) {
                Some(previous) => Some((s, previous)),
                None => {
                    let previous = s.checked_sub(1)?;
                    Some((previous, self.sections[previous].tasks.len()))
                }
            }
        }
    }

    /// `D`: deletes the task without archiving. Undo brings it back.
    pub(super) fn delete(&mut self) -> Result<()> {
        let (s, t) = match self.selected() {
            Some(Row::Task(s, t)) => (s, t),
            Some(Row::Section(s)) => return self.delete_section(s),
            None => return Ok(()),
        };
        // The cursor goes to the next task (like vim), see [`Self::apply`].
        let ops = edit::delete(&self.sections, s, t, &self.state_file);
        self.apply(ops)?;
        self.status = Some(Status::Info("task deleted (u to undo)".to_string()));
        Ok(())
    }

    /// Only empty sections, so `D` never takes tasks with it.
    fn delete_section(&mut self, s: usize) -> Result<()> {
        let section = &self.sections[s];
        let name = section.name.clone();
        if !section.tasks.is_empty() {
            return Err(eyre!("{name} is not empty"));
        }
        // Cursor stays on the same row, which is now the next section.
        let ops = edit::delete_section(&self.sections, s, &self.state_file);
        self.apply(ops)?;
        self.status = Some(Status::Info("section deleted (u to undo)".to_string()));
        Ok(())
    }

    pub(super) fn undo(&mut self) -> Result<()> {
        let Some(step) = self.undo.last() else {
            return Err(eyre!("nothing to undo"));
        };
        // Only pop once the inverse worked, so a failed undo can be retried.
        op::apply_all(&op::inverse_all(&step.ops))?;
        let cursor = step.cursor.clone();
        self.undo.pop();
        self.reload(cursor.as_deref())
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use crate::{
        app::tests::{TODAY, app_with_files, listing, state_file},
        keymap::Action,
        state,
    };

    use super::*;

    #[test]
    fn done_dates_go_where_the_task_goes() {
        let (dir, mut app) = app_with_files(&["work/a.md", "work/b.md", "home/c.md"]);
        let state = "# work\na 2026-10-01\nb\n\n# home\nc 2026-10-02\n";
        fs::write(dir.path().join(state::FILE), state).unwrap();
        app.reload(None).unwrap();

        // Into the next section: the line moves along.
        app.cursor = 1;
        app.update(Action::MoveDown);
        app.update(Action::MoveDown);
        assert_eq!(
            state_file(&dir),
            "# work\nb\n\n# home\na 2026-10-01\nc 2026-10-02\n"
        );

        // A new title renames the file, the date follows on the next write.
        fs::write(dir.path().join("home/c.md"), "# d").unwrap();
        app.reload_keep_selection().unwrap();
        app.update(Action::MoveDown);
        assert_eq!(
            state_file(&dir),
            "# work\nb\n\n# home\nd 2026-10-02\na 2026-10-01\n"
        );

        // Deleted, its date goes with it.
        app.update(Action::Delete);
        assert_eq!(state_file(&dir), "# work\nb\n\n# home\nd 2026-10-02\n");
    }

    #[test]
    fn delete_and_undo() {
        let (dir, mut app) = app_with_files(&["work/a.md", "work/b.md"]);
        app.cursor = 1;

        app.update(Action::Delete);
        assert_eq!(listing(&app), ["[work]", "b.md"]);
        assert_eq!(state_file(&dir), "# work\nb\n");
        assert_eq!(app.cursor, 1, "cursor moves onto the next task");
        assert_eq!(
            app.status,
            Some(Status::Info("task deleted (u to undo)".to_string()))
        );

        app.update(Action::Undo);
        assert_eq!(listing(&app), ["[work]", "a.md", "b.md"]);
        assert_eq!(state_file(&dir), "# work\na\nb\n");
        assert_eq!(app.cursor, 1, "cursor is on the restored task");

        app.update(Action::Undo);
        assert_eq!(
            app.status,
            Some(Status::Error("nothing to undo".to_string()))
        );
    }

    #[test]
    fn delete_section_only_if_empty() {
        let (dir, mut app) = app_with_files(&["work/a.md", "home/", "x/b.md"]);
        let before = listing(&app);

        app.update(Action::Delete);
        assert_eq!(
            app.status,
            Some(Status::Error("work is not empty".to_string()))
        );
        assert_eq!(listing(&app), before);

        // The empty `home` goes, `x` moves up into its place, under the cursor.
        app.cursor = 2;
        app.update(Action::Delete);
        assert_eq!(listing(&app), ["[work]", "a.md", "[x]", "b.md"]);
        assert!(!dir.path().join("home").exists());
        assert_eq!(state_file(&dir), "# work\na\n\n# x\nb\n");
        assert_eq!(app.selected(), Some(Row::Section(1)));
        assert_eq!(
            app.status,
            Some(Status::Info("section deleted (u to undo)".to_string()))
        );

        app.update(Action::Undo);
        assert_eq!(listing(&app), before);
        assert_eq!(app.cursor, 2, "back on `home`");
    }

    #[test]
    fn move_task_within_and_across_sections() {
        let (dir, mut app) = app_with_files(&["work/a.md", "work/b.md", "home/c.md"]);
        app.cursor = 1;

        app.update(Action::MoveDown);
        assert_eq!(listing(&app), ["[work]", "b.md", "a.md", "[home]", "c.md"]);
        assert_eq!(state_file(&dir), "# work\nb\na\n\n# home\nc\n");
        assert_eq!(app.cursor, 2, "cursor follows the task");

        app.update(Action::MoveDown);
        assert_eq!(listing(&app), ["[work]", "b.md", "[home]", "a.md", "c.md"]);
        assert!(dir.path().join("home/a.md").exists());
        assert_eq!(app.cursor, 3);

        // One undo per move, the cursor goes back with the task.
        app.update(Action::Undo);
        assert_eq!(listing(&app), ["[work]", "b.md", "a.md", "[home]", "c.md"]);
        assert_eq!(app.cursor, 2);
        app.update(Action::Undo);
        assert_eq!(app.cursor, 1);
        assert_eq!(listing(&app)[1], "a.md");
    }

    #[test]
    fn move_task_stops_at_the_ends() {
        let (_dir, mut app) = app_with_files(&["work/a.md", "home/b.md"]);
        app.cursor = 1;
        app.update(Action::MoveUp);
        app.cursor = 3;
        app.update(Action::MoveDown);
        assert_eq!(listing(&app), ["[work]", "a.md", "[home]", "b.md"]);
        assert_eq!(app.status, None);
    }

    #[test]
    fn move_section_with_its_tasks() {
        let (dir, mut app) = app_with_files(&["work/a.md", "home/b.md", "x/"]);

        app.update(Action::MoveDown);
        assert_eq!(listing(&app), ["[home]", "b.md", "[work]", "a.md", "[x]"]);
        assert_eq!(app.cursor, 2, "cursor follows the section");
        assert_eq!(state_file(&dir), "# home\nb\n\n# work\na\n\n# x\n");

        app.update(Action::Undo);
        assert_eq!(app.sections[0].name, "work");
        assert_eq!(app.cursor, 0, "cursor goes back with the section");

        // Nothing above the first one.
        app.update(Action::MoveUp);
        assert_eq!(app.sections[0].name, "work");
        assert_eq!(app.status, None);
    }

    #[test]
    fn done_archives_and_undo_restores() {
        let (dir, mut app) = app_with_files(&["work/a.md", "home/b.md"]);
        fs::write(dir.path().join("work/a.md"), "# a\n\nnotes\n").unwrap();
        let archive = dir.path().join(".archive");
        let day = archive.join(TODAY.to_string());

        app.cursor = 1;
        app.update(Action::Done);
        app.cursor = 2;
        app.update(Action::Done);
        assert_eq!(
            app.status,
            Some(Status::Info("task archived (u to undo)".to_string()))
        );
        assert_eq!(app.done_today().len(), 2);
        assert_eq!(listing(&app), ["[work]", "[home]"]);
        assert_eq!(state_file(&dir), "# work\n\n# home\n");
        assert_eq!(
            fs::read_to_string(day.join("work/a.md")).unwrap(),
            "# a\n\nnotes\n",
            "moved as it was"
        );
        assert!(day.join("home/b.md").exists());

        app.update(Action::Undo);
        assert_eq!(listing(&app), ["[work]", "[home]", "b.md"]);
        assert_eq!(app.done_today().len(), 1);
        assert!(
            !day.join("home").exists(),
            "the directory it needed is gone"
        );

        app.update(Action::Undo);
        assert_eq!(listing(&app), ["[work]", "a.md", "[home]", "b.md"]);
        assert!(!archive.exists());
    }

    #[test]
    fn done_twice_a_day_gets_a_number() {
        let (dir, mut app) = app_with_files(&["work/a.md"]);
        let day = dir.path().join(".archive").join(TODAY.to_string());
        app.cursor = 1;
        app.update(Action::Done);
        // The same task again, written outside markoff.
        fs::write(dir.path().join("work/a.md"), "").unwrap();
        app.reload(None).unwrap();

        app.cursor = 1;
        app.update(Action::Done);
        assert!(day.join("work/a.md").exists());
        assert!(day.join("work/a-2.md").exists());
        assert_eq!(app.done_today().len(), 2);
    }

    #[test]
    fn cancel_archives_struck_through() {
        let (dir, mut app) = app_with_files(&["work/a.md", "work/water.md"]);
        fs::write(dir.path().join("work/a.md"), "# A\n\nnotes\n").unwrap();
        fs::write(dir.path().join("work/water.md"), "# Water `* * *`\n").unwrap();
        app.reload(None).unwrap();
        let archived = dir
            .path()
            .join(".archive")
            .join(TODAY.to_string())
            .join("work/a.md");

        app.cursor = 1;
        app.update(Action::Cancel);
        assert_eq!(
            app.status,
            Some(Status::Info("task canceled (u to undo)".to_string()))
        );
        assert_eq!(listing(&app), ["[work]", "water.md"]);
        assert_eq!(fs::read_to_string(&archived).unwrap(), "# ~~A~~\n\nnotes\n");
        assert!(app.done_today()[0].task.canceled);

        // A repeating task is the same as done.
        app.update(Action::Cancel);
        assert_eq!(listing(&app), ["[work]"]);
        assert_eq!(state_file(&dir), format!("# work\nwater {TODAY}\n"));

        // One undo step each, the file as it was.
        app.update(Action::Undo);
        app.update(Action::Undo);
        assert_eq!(listing(&app), ["[work]", "a.md", "water.md"]);
        assert_eq!(app.selected(), Some(Row::Task(0, 0)), "back on it");
        let content = fs::read_to_string(dir.path().join("work/a.md")).unwrap();
        assert_eq!(content, "# A\n\nnotes\n");
        assert!(!archived.exists());
    }

    #[test]
    fn repeating_task_done_until_next_time() {
        let (dir, mut app) = app_with_files(&["work/a.md", "work/water.md"]);
        let water = "# Water `* * *`\n";
        fs::write(dir.path().join("work/water.md"), water).unwrap();
        app.reload(None).unwrap();

        // Never done: due.
        app.cursor = 2;
        app.update(Action::Done);
        assert_eq!(
            app.status,
            Some(Status::Info("done until next time (u to undo)".to_string()))
        );
        // Hidden, the date in the state file, the task and the archive untouched.
        assert_eq!(listing(&app), ["[work]", "a.md"]);
        assert_eq!(state_file(&dir), format!("# work\na\nwater {TODAY}\n"));
        let content = fs::read_to_string(dir.path().join("work/water.md")).unwrap();
        assert_eq!(content, water);
        assert!(!dir.path().join(".archive").exists());
        assert_eq!(app.done_today().len(), 0, "not in the archive");

        // Again while `.` shows it: nothing changes, no undo step to take back.
        app.update(Action::ShowHidden);
        app.cursor = 2;
        let steps = app.undo.len();
        app.update(Action::Done);
        assert_eq!(app.undo.len(), steps);
        assert_eq!(
            app.status,
            Some(Status::Info("hidden tasks shown (1)".to_string())),
            "no claim"
        );
        assert_eq!(state_file(&dir), format!("# work\na\nwater {TODAY}\n"));
        app.update(Action::ShowHidden);

        app.update(Action::Undo);
        assert_eq!(listing(&app), ["[work]", "a.md", "water.md"]);
        assert_eq!(app.selected(), Some(Row::Task(0, 1)), "back on it");
        assert_eq!(state_file(&dir), "# work\na\nwater\n");

        // Done yesterday: due again today.
        fs::write(
            dir.path().join(state::FILE),
            format!("# work\na\nwater {}\n", TODAY.yesterday().unwrap()),
        )
        .unwrap();
        app.reload(None).unwrap();
        assert_eq!(listing(&app), ["[work]", "a.md", "water.md"]);
    }
}
