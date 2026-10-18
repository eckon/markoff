//! The archive page: the cursor in the heatmap. Read-only, nothing here writes.

use jiff::civil::Date;

use super::App;
use crate::{history, keymap::Action};

/// Where the archive page is, kept while switching pages.
#[derive(Debug, Default)]
pub struct ArchivePage {
    /// A day in the selected heatmap cell. A day and not a cell: after a resize the cells are
    /// others, the cursor lands on the one that holds it. `None` until the page is first opened.
    pub day: Option<Date>,
    /// Heatmap columns in the last frame, the cursor moves over these.
    pub columns: usize,
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

    /// Runs `action` on the archive page. `false` if it is none of the page's own.
    pub(super) fn update_archive(&mut self, action: Action) -> bool {
        let by = match action {
            Action::Left => (-1, 0),
            Action::Right => (1, 0),
            Action::Up => (0, -1),
            Action::Down => (0, 1),
            _ => return false,
        };
        let today = jiff::Zoned::now().date();
        let columns = history::columns(&self.days, today, self.archive.columns);
        self.archive.day = Some(history::step(&columns, self.archive_day(), by, today));
        true
    }
}
