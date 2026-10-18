//! The done tasks of the selected heatmap cell, below the overview. Laid out like the todo
//! list: section rules with their keys, a bullet per task, its day on the right. The day or the
//! days of the cell are in the header, see [`range`].

use jiff::{ToSpan, civil::Date};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Style, Stylize},
    text::Span,
};

use crate::{
    app::{
        App,
        archive::{ArchiveRow, Focus},
    },
    ui::{list_with_preview, rows, section_color},
};

/// The list below the overview, see [`rows`], with the preview of the task at the cursor like
/// in the todo list, unless `p` hid it. Returns the scroll offset and where the preview went.
pub fn render(app: &App, frame: &mut Frame, area: Rect, columns: usize) -> (usize, Option<Rect>) {
    let rows = app.archive_rows(columns);
    // Only while the list has the keys: moving in the heatmap would open and close it.
    let hidden = app.archive.hide_preview || app.archive.focus == Focus::Heatmap;
    let body = match rows.get(app.archive.cursor) {
        _ if hidden => None,
        Some(&ArchiveRow::Task(_, day, index)) => Some(app.done[&day][index].body.as_str()),
        _ => None,
    };
    let list = |frame: &mut Frame, area, width| render_rows(app, frame, area, &rows, width);
    list_with_preview(frame, area, body, "", list)
}

/// The rows themselves, the rules `width` wide.
fn render_rows(
    app: &App,
    frame: &mut Frame,
    area: Rect,
    rows: &[ArchiveRow],
    width: usize,
) -> usize {
    let today = jiff::Zoned::now().date();
    let sections = app.archive_sections();
    // Keys count the sections shown, the colors stay those of the todo list.
    let mut keys = 0..;
    let items: Vec<rows::Row> = (rows.iter().enumerate())
        .map(|(i, &row)| {
            let (ArchiveRow::Section(s) | ArchiveRow::Task(s, ..)) = row;
            let color = section_color(s);
            let kind = match row {
                ArchiveRow::Section(s) => {
                    let count = rows
                        .iter()
                        .filter(|row| matches!(row, ArchiveRow::Task(task_s, ..) if *task_s == s))
                        .count();
                    let key = keys.next().unwrap_or_default();
                    let rule = rows::section_rule(
                        key,
                        color,
                        &sections[s],
                        count,
                        width.saturating_sub(1),
                    );
                    rows::Kind::Header(rule)
                }
                ArchiveRow::Task(_, day, index) => {
                    let style = if i == app.archive.cursor {
                        Style::new().fg(color)
                    } else {
                        Style::new()
                    };
                    let title = Span::styled(app.done[&day][index].title.clone(), style);
                    rows::Kind::Task(vec![title], Some(when(day, today).dim()))
                }
            };
            rows::Row { color, kind }
        })
        .collect();
    let notes: &[&str] = if items.is_empty() {
        &["nothing done"]
    } else {
        &[]
    };
    let state = [app.archive.cursor, app.archive.offset];
    rows::render(frame, area, items, state, notes)
}

/// The day of the selected cell for the header, `Fri 16 Oct 2026`, a cell of more days as
/// `02 Mar 2026 – 15 Mar 2026`.
pub fn range(app: &App, columns: usize) -> String {
    let (start, length) = app.archive_cell(columns);
    match length {
        1 => start.strftime("%a %d %b %Y").to_string(),
        _ => {
            let end = start + (length - 1).days();
            format!(
                "{} – {}",
                start.strftime("%d %b %Y"),
                end.strftime("%d %b %Y")
            )
        }
    }
}

/// The day a task was done, as near days are said: `today`, `yesterday`, `Mon 13` within a
/// week, further back the date.
fn when(day: Date, today: Date) -> String {
    match (today - day).get_days() {
        0 => "today".to_string(),
        1 => "yesterday".to_string(),
        2..=6 => day.strftime("%a %d").to_string(),
        _ => day.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn near_days_by_name() {
        let today: Date = "2026-10-18".parse().unwrap();
        let when = |day: &str| when(day.parse().unwrap(), today);
        assert_eq!(when("2026-10-18"), "today");
        assert_eq!(when("2026-10-17"), "yesterday");
        assert_eq!(when("2026-10-12"), "Mon 12");
        assert_eq!(when("2026-10-11"), "2026-10-11");
    }
}
