//! The done tasks of the selection, below the heatmap. Laid out like the task list: section
//! headers with their keys, a bullet per task, its day on the right if the selection has more
//! than one. The day or the days of the selection are in the app header, see [`range`].

use jiff::{ToSpan, Unit, civil::Date};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Style, Stylize},
};

use crate::{
    app::{
        App,
        archive::{ArchiveRow, Focus},
    },
    ui::{filtered_title, list_with_preview, rows, section_color},
};

/// The list below the heatmap, see [`rows`], with the preview of the selected task like
/// in the task list, unless `p` hid it. Returns the scroll offset and where the preview went.
pub fn render(app: &App, frame: &mut Frame, area: Rect) -> (usize, Option<Rect>) {
    let rows = app.archive_rows();
    // Only while the list has the keys: moving in the heatmap would open and close it.
    let hidden = app.archive_page.hide_preview || app.archive_page.focus == Focus::Heatmap;
    let body = match rows.get(app.archive_page.cursor) {
        _ if hidden => None,
        Some(&ArchiveRow::Task(_, day, index)) => Some(app.archive[&day][index].task.body.as_str()),
        Some(_) => Some(""),
        None => None,
    };
    // One day is in the app header. More need it on each row: in full where there is room, next to
    // the preview only the short date fits, the app header has the rest.
    let (_, length) = app.archive_selection();
    let preview = body.is_some();
    let when = |day: Date| match (length, preview) {
        (1, _) => None,
        (_, true) => Some(day.strftime("%d %b").to_string()),
        (_, false) => Some(when(day, app.today)),
    };
    let list = |frame: &mut Frame, area, width| render_rows(app, frame, area, &rows, width, when);
    list_with_preview(frame, area, body, &app.archive_page.filter, list)
}

/// The rows themselves, the section headers `width` wide.
fn render_rows(
    app: &App,
    frame: &mut Frame,
    area: Rect,
    rows: &[ArchiveRow],
    width: usize,
    when: impl Fn(Date) -> Option<String>,
) -> usize {
    let sections = app.archive_sections();
    // No cursor while the heatmap has the keys, its selected day says what the list shows.
    let cursor = (app.archive_page.focus == Focus::List).then_some(app.archive_page.cursor);
    let items: Vec<rows::Row> = (rows.iter().enumerate())
        .map(|(i, &row)| {
            // The colors stay those of the task list, the keys count the sections shown.
            let (ArchiveRow::Section(s, _) | ArchiveRow::Task(s, ..)) = row;
            let color = section_color(s);
            let kind = match row {
                ArchiveRow::Section(s, key) => {
                    let count = rows
                        .iter()
                        .filter(|row| matches!(row, ArchiveRow::Task(task_s, ..) if *task_s == s))
                        .count();
                    let header = rows::section_header(
                        key,
                        color,
                        &sections[s],
                        count,
                        width.saturating_sub(1),
                    );
                    rows::Kind::Section(header)
                }
                ArchiveRow::Task(_, day, index) => {
                    let style = if cursor == Some(i) {
                        Style::new().fg(color)
                    } else {
                        Style::new()
                    };
                    let title = app.archive[&day][index].task.title();
                    let spans = filtered_title(&title, &app.archive_page.filter, style);
                    rows::Kind::Task(spans, when(day).map(Stylize::dim), false)
                }
            };
            rows::Row { color, kind }
        })
        .collect();
    let notes: &[&str] = match (items.is_empty(), app.archive_page.filter.is_empty()) {
        (false, _) => &[],
        (true, true) => &["<empty>"],
        (true, false) => &["no matches"],
    };
    rows::render(frame, area, items, (cursor, app.archive_page.offset), notes)
}

/// The selected day for the app header with how long ago, `Fri 16 Oct 2026 · today`, a
/// selection of more days as `02 Mar 2026 – 15 Mar 2026`.
pub fn range(app: &App) -> String {
    let (start, length) = app.archive_selection();
    match length {
        1 => {
            let date = start.strftime("%a %d %b %Y").to_string();
            match ago(start, app.today) {
                Some(ago) => format!("{date} · {ago}"),
                None => date,
            }
        }
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

/// The day a task was done with how long ago: `3 days ago · Mon 19 Oct`, with the year if it is
/// not this one.
fn when(day: Date, today: Date) -> String {
    let format = match day.year() == today.year() {
        true => "%a %d %b",
        false => "%a %d %b %Y",
    };
    let date = day.strftime(format).to_string();
    match ago(day, today) {
        Some(ago) => format!("{ago} · {date}"),
        None => date,
    }
}

/// How long ago `day` was, in the largest whole unit: `today`, `3 days ago`, `2 weeks ago`.
/// `None` after today, which only an archive edited by hand has.
fn ago(day: Date, today: Date) -> Option<String> {
    let days = (today - day).get_days();
    let months = (day.until((Unit::Month, today))).map_or(0, |span| span.get_months());
    let ago = match days {
        ..0 => return None,
        0 => "today".to_string(),
        1 => "yesterday".to_string(),
        2..=13 => format!("{days} days ago"),
        // Under two months is at most 61 days, so 2 to 8 weeks.
        _ if months < 2 => format!("{} weeks ago", days / 7),
        _ if months < 24 => format!("{months} months ago"),
        _ => format!("{} years ago", months / 12),
    };
    Some(ago)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn how_long_ago_in_the_largest_whole_unit() {
        let today: Date = "2026-10-18".parse().unwrap();
        let when = |day: &str| when(day.parse().unwrap(), today);
        assert_eq!(when("2026-10-18"), "today · Sun 18 Oct");
        assert_eq!(when("2026-10-17"), "yesterday · Sat 17 Oct");
        assert_eq!(when("2026-10-05"), "13 days ago · Mon 05 Oct");
        assert_eq!(when("2026-10-04"), "2 weeks ago · Sun 04 Oct");
        assert_eq!(when("2026-08-19"), "8 weeks ago · Wed 19 Aug");
        assert_eq!(when("2026-08-18"), "2 months ago · Tue 18 Aug");
        assert_eq!(when("2024-10-19"), "23 months ago · Sat 19 Oct 2024");
        assert_eq!(when("2024-10-18"), "2 years ago · Fri 18 Oct 2024");
        assert_eq!(when("2026-10-19"), "Mon 19 Oct", "after today");
    }
}
