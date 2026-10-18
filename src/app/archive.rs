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
    /// The stats below the heatmap are shown, off until `s`.
    pub show_stats: bool,
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
            let last = self.days.range(..=today).next_back();
            last.map_or(today, |(&day, _)| day)
        })
    }

    /// Runs `action` on the archive page. `false` for those of every page, the rest stays here
    /// and never reaches the todo list behind it.
    pub(super) fn update_archive(&mut self, action: Action) -> bool {
        let by = match (action, self.archive.focus) {
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
            (Action::Stats, _) => {
                self.archive.show_stats = !self.archive.show_stats;
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
            (Action::Left, _) => (-1, 0),
            (Action::Right, _) => (1, 0),
            (Action::Up, _) => (0, -1),
            (Action::Down, _) => (0, 1),
            _ => return true,
        };
        let today = jiff::Zoned::now().date();
        let columns = history::columns(&self.days, today, self.archive.columns);
        let day = history::step(&columns, self.archive_day(), by, today);
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
