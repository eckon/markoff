//! The archive page: the cursor in the heatmap and the done tasks of its cell. Read-only,
//! nothing here writes.

use std::collections::BTreeSet;

use jiff::{ToSpan, civil::Date};

use super::App;
use crate::{cursor, heatmap, keymap::Action};

/// Where the archive page is, kept while switching pages.
#[derive(Debug, Default)]
pub struct ArchivePage {
    /// A day in the selected heatmap cell. A day and not a cell: after a resize the cells are
    /// others, the cursor lands on the one that holds it. `None` until the page is first opened.
    pub day: Option<Date>,
    /// Heatmap columns in the last frame, the cursor moves over these.
    pub columns: usize,
    /// Index into [`App::archive_rows`].
    pub cursor: usize,
    /// First list line on screen, like [`App::list_offset`].
    pub offset: usize,
    pub focus: Focus,
    /// `p` hid the preview next to the list, its own and not the task list's.
    pub hide_preview: bool,
}

/// What takes the move keys on the archive page, switched with `<CR>` / `Esc`.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    #[default]
    Heatmap,
    List,
}

/// One line in the list of done tasks, grouped like the task list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchiveRow {
    /// Index into [`App::archive_sections`], for its color, and its key: the position among the
    /// sections shown.
    Section(usize, usize),
    /// The section, the day and the task's index in [`App::done`] on that day.
    Task(usize, Date, usize),
}

impl App {
    /// The selected day, see [`ArchivePage::day`]. Before the page was opened the last day with
    /// tasks done, so a Monday shows Friday. Today if nothing is done yet.
    pub fn archive_day(&self) -> Date {
        let today = self.today;
        self.archive_page.day.unwrap_or_else(|| {
            // A day directory can be empty, that is no day with tasks done.
            let last = self.days.range(..=today).rfind(|(_, done)| **done > 0);
            last.map_or(today, |(&day, _)| day)
        })
    }

    /// Runs `action` on the archive page. `false` for those of every page, the rest stays here
    /// and never reaches the task list behind it.
    pub(super) fn update_archive(&mut self, action: Action) -> bool {
        let (by, skip) = match (action, self.archive_page.focus) {
            (Action::Help | Action::PrevPage | Action::NextPage | Action::Quit, _) => {
                return false;
            }
            (Action::FocusList, _) => {
                self.archive_page.focus = Focus::List;
                return true;
            }
            (Action::FocusHeatmap, _) => {
                self.archive_page.focus = Focus::Heatmap;
                return true;
            }
            (Action::Preview, Focus::List) => {
                self.archive_page.hide_preview = !self.archive_page.hide_preview;
                return true;
            }
            // Like in the task list, `h` / `l` do nothing. A section key from the heatmap goes
            // to the list, like `<CR>` and the jump in one.
            (_, Focus::List) | (Action::JumpSection(_), Focus::Heatmap) => {
                let lines: Vec<cursor::Line> = (self.archive_rows(self.archive_page.columns))
                    .into_iter()
                    .map(|row| match row {
                        ArchiveRow::Section(_, key) => cursor::Line::Section(key),
                        ArchiveRow::Task(..) => cursor::Line::Task,
                    })
                    .collect();
                // Only to a section that is there, otherwise the heatmap keeps the keys.
                if let Action::JumpSection(s) = action
                    && !lines.contains(&cursor::Line::Section(s))
                {
                    return true;
                }
                self.archive_page.focus = Focus::List;
                self.archive_page.cursor = cursor::moved(&lines, self.archive_page.cursor, action);
                return true;
            }
            (Action::Left, _) => ((-1, 0), false),
            (Action::Right, _) => ((1, 0), false),
            (Action::Up, _) => ((0, -1), false),
            (Action::Down, _) => ((0, 1), false),
            (Action::SkipLeft, _) => ((-1, 0), true),
            (Action::SkipRight, _) => ((1, 0), true),
            (Action::SkipUp, _) => ((0, -1), true),
            (Action::SkipDown, _) => ((0, 1), true),
            _ => return true,
        };
        let columns = heatmap::columns(&self.days, self.today, self.archive_page.columns);
        let day = heatmap::step(
            &columns,
            &self.days,
            self.archive_day(),
            by,
            skip,
            self.today,
        );
        if Some(day) != self.archive_page.day {
            self.select_day(day);
        }
        true
    }

    /// The first time the page opens, see [`Self::archive_day`]. Later it stays where it was.
    pub(super) fn open_archive(&mut self) {
        if self.archive_page.day.is_none() {
            self.select_day(self.archive_day());
        }
    }

    /// Moves the heatmap cursor to `day`. A new list, on its first task like a jump to a
    /// section.
    fn select_day(&mut self, day: Date) {
        self.archive_page.day = Some(day);
        let rows = self.archive_rows(self.archive_page.columns);
        self.archive_page.cursor = usize::from(!rows.is_empty());
        self.archive_page.offset = 0;
    }

    /// The sections of the done tasks: those of the task list in its order, so they keep their
    /// key and color, then the ones only in the archive by name.
    pub fn archive_sections(&self) -> Vec<String> {
        let mut names: Vec<String> = self.sections.iter().map(|s| s.name.clone()).collect();
        let gone: BTreeSet<&String> = self
            .archive
            .values()
            .flatten()
            .map(|done| &done.section)
            .filter(|name| !names.contains(name))
            .collect();
        names.extend(gone.into_iter().cloned());
        names
    }

    /// The first day of the selected cell and its days, in a heatmap of `columns` columns.
    pub fn archive_cell(&self, columns: usize) -> (Date, i64) {
        let columns = heatmap::columns(&self.days, self.today, columns);
        let day = self.archive_day();
        match heatmap::find(&columns, day) {
            Some((column, row)) => columns[column].cell(row),
            None => (day, 1),
        }
    }

    /// The done tasks of the selected cell under their sections, newest day first, by name within
    /// a day. Sections without any are left out.
    pub fn archive_rows(&self, columns: usize) -> Vec<ArchiveRow> {
        let (start, length) = self.archive_cell(columns);
        let days: Vec<_> = self
            .archive
            .range(start..start + length.days())
            .rev()
            .collect();
        let mut rows = Vec::new();
        let mut key = 0;
        for (s, name) in self.archive_sections().iter().enumerate() {
            let tasks: Vec<ArchiveRow> = days
                .iter()
                .flat_map(|&(&day, done)| {
                    (0..done.len())
                        .filter(|&i| &done[i].section == name)
                        .map(move |i| ArchiveRow::Task(s, day, i))
                })
                .collect();
            if !tasks.is_empty() {
                rows.push(ArchiveRow::Section(s, key));
                rows.extend(tasks);
                key += 1;
            }
        }
        rows
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::{
        app::tests::{TODAY, sections},
        archive::Done,
        task::Task,
    };

    #[test]
    fn first_open_skips_empty_days() {
        let mut app = App::new(PathBuf::new(), sections(&[("work", &[])]));
        app.today = TODAY;
        // Far back, where a cell covers several days, the newest day without tasks.
        let day = TODAY - 400.days();
        app.archive = [
            (day - 1.days(), vec![done("work")]),
            (day, vec![done("work")]),
            (day + 1.days(), Vec::new()),
        ]
        .into();
        app.days = heatmap::days(&app.archive);

        // Before the first frame the heatmap's columns are not known yet.
        app.update(Action::NextPage);
        assert_eq!(app.archive_day(), day);
        // In the frame the cell covers all three days, the cursor is on the newest task.
        let rows = app.archive_rows(40);
        assert!(app.archive_cell(40).1 > 1);
        assert_eq!(rows[app.archive_page.cursor], ArchiveRow::Task(0, day, 0));
    }

    /// A done task in `section`.
    fn done(section: &str) -> Done {
        Done {
            section: section.to_string(),
            task: Task::parse(PathBuf::new(), ""),
        }
    }

    #[test]
    fn heatmap_moves_by_cell_and_skips_empty_ones() {
        // A Wednesday three weeks back, so up and down stay in its column. A short archive,
        // every cell is a day.
        let day = jiff::civil::date(2026, 9, 30);
        let mut app = App::new(PathBuf::new(), sections(&[("work", &[])]));
        app.today = jiff::civil::date(2026, 10, 21);
        app.archive = [day - 28.days(), day, day + 2.days(), day + 14.days()]
            .map(|day| (day, vec![done("work")]))
            .into();
        app.days = heatmap::days(&app.archive);
        app.archive_page.columns = 40;
        app.update(Action::NextPage);
        assert_eq!(app.archive_day(), day + 14.days());
        let mut press = |action, expected: Date| {
            app.update(action);
            assert_eq!(app.archive_day(), expected, "{action:?}");
            // A new cell puts the list on its first task, if it has one.
            assert_eq!(
                app.archive_page.cursor,
                usize::from(app.days.contains_key(&expected))
            );
        };

        // A week to the side, a day up and down, empty cells too.
        press(Action::Left, day + 7.days());
        press(Action::Left, day);
        press(Action::Down, day + 1.days());
        press(Action::Up, day);
        // Skipping goes on to the next cell with tasks done that way, stays if there is none.
        press(Action::SkipDown, day + 2.days());
        press(Action::SkipUp, day);
        press(Action::SkipRight, day + 14.days());
        press(Action::SkipRight, day + 14.days());
        press(Action::SkipLeft, day);
        press(Action::SkipLeft, day - 28.days());
        // The first column is the edge, after today there is nothing.
        press(Action::Left, day - 28.days());
        app.archive_page.day = Some(app.today);
        app.update(Action::Down);
        assert_eq!(app.archive_day(), app.today);
    }

    #[test]
    fn list_takes_the_keys_with_cr_or_a_section_key() {
        let mut app = App::new(
            PathBuf::new(),
            sections(&[("work", &[]), ("home", &[]), ("misc", &[])]),
        );
        app.today = TODAY;
        app.archive = [(TODAY, vec![done("work"), done("misc"), done("misc")])].into();
        app.days = heatmap::days(&app.archive);
        app.update(Action::NextPage);
        // Without tasks `home` is not in the list, the keys count the sections shown.
        let rows = app.archive_rows(app.archive_page.columns);
        assert_eq!(rows[2], ArchiveRow::Section(2, 1));

        // A key without a section shown does nothing, the heatmap keeps the keys.
        app.update(Action::JumpSection(2));
        assert_eq!(
            (app.archive_page.focus, app.archive_page.cursor),
            (Focus::Heatmap, 1)
        );
        // `2` goes to `misc`'s first task in the list.
        app.update(Action::JumpSection(1));
        assert_eq!(
            (app.archive_page.focus, app.archive_page.cursor),
            (Focus::List, 3)
        );
        app.update(Action::Down);
        assert_eq!(app.archive_page.cursor, 4);
        // `h` / `l` do nothing in the list, the day stays.
        app.update(Action::Left);
        assert_eq!((app.archive_day(), app.archive_page.cursor), (TODAY, 4));

        // `Esc` gives the keys back to the heatmap, `<CR>` to the list, the list cursor stays.
        // Twice changes nothing.
        app.update(Action::FocusHeatmap);
        app.update(Action::FocusHeatmap);
        assert_eq!(app.archive_page.focus, Focus::Heatmap);
        app.update(Action::FocusList);
        app.update(Action::FocusList);
        assert_eq!(
            (app.archive_page.focus, app.archive_page.cursor),
            (Focus::List, 4)
        );
        app.update(Action::Up);
        assert_eq!(app.archive_page.cursor, 3);
    }
}
