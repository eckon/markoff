//! The archive page: the cursor in the heatmap and the done tasks of its cell. Read-only,
//! nothing here writes.

use std::collections::BTreeSet;

use jiff::{ToSpan, civil::Date};

use super::App;
use crate::{cursor, history, keymap::Action};

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
    /// `p` hid the preview next to the list, its own and not the todo list's.
    pub hide_preview: bool,
}

/// What takes the move keys on the archive page, switched with `Tab`.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    #[default]
    Heatmap,
    List,
}

/// One line in the list of done tasks, grouped like the todo list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchiveRow {
    /// Index into [`App::archive_sections`], for its color. Its key counts the sections shown.
    Section(usize),
    /// The section, the day and the task's index in [`App::done`] on that day.
    Task(usize, Date, usize),
}

impl App {
    /// The selected day, see [`ArchivePage::day`]. Before the page was opened the last day with
    /// tasks done, so a Monday shows Friday. Today if nothing is done yet.
    pub fn archive_day(&self) -> Date {
        let today = jiff::Zoned::now().date();
        self.archive.day.unwrap_or_else(|| {
            // An archive file can be empty, that is no day with tasks done.
            let last = self.days.range(..=today).rfind(|(_, done)| **done > 0);
            last.map_or(today, |(&day, _)| day)
        })
    }

    /// Runs `action` on the archive page. `false` for those of every page, the rest stays here
    /// and never reaches the todo list behind it.
    pub(super) fn update_archive(&mut self, action: Action) -> bool {
        let (by, skip) = match (action, self.archive.focus) {
            (Action::Help | Action::PrevPage | Action::NextPage | Action::Quit, _) => {
                return false;
            }
            (Action::Focus, Focus::Heatmap) => {
                self.archive.focus = Focus::List;
                return true;
            }
            (Action::Focus, Focus::List) => {
                self.archive.focus = Focus::Heatmap;
                return true;
            }
            (Action::Preview, Focus::List) => {
                self.archive.hide_preview = !self.archive.hide_preview;
                return true;
            }
            // Like in the todo list, `h` / `l` do nothing. A section key from the heatmap goes
            // to the list, like `Tab` and the jump in one.
            (_, Focus::List) | (Action::JumpSection(_), Focus::Heatmap) => {
                let mut keys = 0..;
                let lines: Vec<cursor::Line> = (self.archive_rows(self.archive.columns))
                    .into_iter()
                    .map(|row| match row {
                        ArchiveRow::Section(_) => cursor::Line::Header(keys.next()),
                        ArchiveRow::Task(..) => cursor::Line::Item,
                    })
                    .collect();
                // Only to a section that is there, otherwise the heatmap keeps the keys.
                if let Action::JumpSection(s) = action
                    && !lines.contains(&cursor::Line::Header(Some(s)))
                {
                    return true;
                }
                self.archive.focus = Focus::List;
                self.archive.cursor = cursor::moved(&lines, self.archive.cursor, action);
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
        let today = jiff::Zoned::now().date();
        let columns = history::columns(&self.days, today, self.archive.columns);
        let day = history::step(&columns, &self.days, self.archive_day(), by, skip, today);
        if Some(day) != self.archive.day {
            self.select_day(day);
        }
        true
    }

    /// The first time the page opens, see [`Self::archive_day`]. Later it stays where it was.
    pub(super) fn open_archive(&mut self) {
        if self.archive.day.is_none() {
            self.select_day(self.archive_day());
        }
    }

    /// Moves the heatmap cursor to `day`. A new list, on its first task like a jump to a
    /// section.
    fn select_day(&mut self, day: Date) {
        self.archive.day = Some(day);
        let rows = self.archive_rows(self.archive.columns);
        self.archive.cursor = usize::from(!rows.is_empty());
        self.archive.offset = 0;
    }

    /// The sections of the done tasks: those of the todo list in its order, so they keep their
    /// key and color, then the ones only in the archive by name.
    pub fn archive_sections(&self) -> Vec<String> {
        let mut names: Vec<String> = self.sections.iter().map(|s| s.name.clone()).collect();
        let gone: BTreeSet<&String> = self
            .done
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
        let today = jiff::Zoned::now().date();
        let columns = history::columns(&self.days, today, columns);
        let day = self.archive_day();
        match history::find(&columns, day) {
            Some((column, row)) => columns[column].cell(row),
            None => (day, 1),
        }
    }

    /// The done tasks of the selected cell under their sections, newest first. Sections without
    /// any are left out.
    pub fn archive_rows(&self, columns: usize) -> Vec<ArchiveRow> {
        let (start, length) = self.archive_cell(columns);
        let days: Vec<_> = self
            .done
            .range(start..start + length.days())
            .rev()
            .collect();
        let mut rows = Vec::new();
        for (s, name) in self.archive_sections().iter().enumerate() {
            let tasks: Vec<ArchiveRow> = days
                .iter()
                .flat_map(|&(&day, done)| {
                    (0..done.len())
                        .rev()
                        .filter(|&i| &done[i].section == name)
                        .map(move |i| ArchiveRow::Task(s, day, i))
                })
                .collect();
            if !tasks.is_empty() {
                rows.push(ArchiveRow::Section(s));
                rows.extend(tasks);
            }
        }
        rows
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::{app::tests::sections, archive::Done};

    #[test]
    fn first_open_skips_empty_archive_files() {
        let today = jiff::Zoned::now().date();
        let mut app = App::new(PathBuf::new(), sections(&[("work", &[])]));
        let done = |title: &str| Done {
            section: "work".to_string(),
            title: title.to_string(),
            body: String::new(),
        };
        // Far back, where a cell covers several days, the newest file without tasks.
        let day = today - 400.days();
        app.done = [
            (day - 1.days(), vec![done("Older")]),
            (day, vec![done("Newest")]),
            (day + 1.days(), Vec::new()),
        ]
        .into();
        app.days = history::days(&app.done);

        // Before the first frame the heatmap's columns are not known yet.
        app.update(Action::NextPage);
        assert_eq!(app.archive_day(), day);
        // In the frame the cell covers all three days, the cursor is on the newest task.
        let rows = app.archive_rows(40);
        assert!(app.archive_cell(40).1 > 1);
        assert_eq!(rows[app.archive.cursor], ArchiveRow::Task(0, day, 0));
    }
}
