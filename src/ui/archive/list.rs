//! The done tasks of the selected heatmap cell, below the overview. Laid out like the todo
//! list: section rules with their keys, a bullet per task, its day on the right.

use jiff::{ToSpan, civil::Date};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Style, Stylize},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, List, ListItem, ListState, Padding},
};

use crate::{
    app::{App, archive::ArchiveRow},
    ui::{LINE_COLOR, section_color, section_rule, task_prefix, truncate},
};

/// The list in a frame below the overview, which draws the line above it. Returns the scroll
/// offset.
pub fn render(app: &App, frame: &mut Frame, area: Rect, columns: usize) -> usize {
    let block = Block::new()
        .borders(Borders::LEFT | Borders::RIGHT | Borders::BOTTOM)
        .border_type(BorderType::Rounded)
        .border_style(LINE_COLOR)
        .padding(Padding::horizontal(1));
    let width = block.inner(area).width as usize;
    let today = jiff::Zoned::now().date();
    let sections = app.archive_sections();
    let rows = app.archive_rows(columns);

    let mut items: Vec<ListItem> = Vec::new();
    let mut selected = 0;
    for (i, &row) in rows.iter().enumerate() {
        let is_cursor = i == app.archive.cursor;
        let (ArchiveRow::Section(s) | ArchiveRow::Task(s, ..)) = row;
        let color = section_color(s);
        // Blank line before each section except the first, the cursor skips it.
        if i > 0 && matches!(row, ArchiveRow::Section(_)) {
            items.push(ListItem::new(""));
        }
        if is_cursor {
            selected = items.len();
        }
        let gutter = || -> Span<'static> {
            if is_cursor {
                "▌".fg(color)
            } else {
                " ".into()
            }
        };
        let line = match row {
            ArchiveRow::Section(s) => {
                let count = rows
                    .iter()
                    .filter(|row| matches!(row, ArchiveRow::Task(task_s, ..) if *task_s == s))
                    .count();
                let mut spans = vec![gutter()];
                spans.extend(section_rule(
                    s,
                    &sections[s],
                    count,
                    width.saturating_sub(gutter().width()),
                ));
                spans
            }
            ArchiveRow::Task(_, day, index) => {
                let style = if is_cursor {
                    Style::new().fg(color)
                } else {
                    Style::new()
                };
                let title = app.done[&day][index].title.clone();
                let when = format!(" {}", when(day, today));
                let mut spans = task_prefix(gutter(), color, true);
                let used = Line::from(spans.clone()).width() + when.chars().count();
                let title = truncate(vec![Span::styled(title, style)], width.saturating_sub(used));
                let title_width: usize = title.iter().map(Span::width).sum();
                spans.extend(title);
                spans.push(" ".repeat(width.saturating_sub(used + title_width)).into());
                spans.push(when.dim());
                spans
            }
        };
        items.push(ListItem::new(Line::from(line)));
    }
    if items.is_empty() {
        // Gutter column like the rows, so it lines up with the section rules.
        items.push(ListItem::new(" nothing done".dim()));
    }

    let mut state = ListState::default()
        .with_offset(app.archive.offset)
        .with_selected(Some(selected));
    frame.render_stateful_widget(List::new(items).block(block), area, &mut state);
    state.offset()
}

/// What the list shows, for the line above it: `Fri 2026-10-16 · 3 tasks`, a cell of more days
/// as `2026-03-02 – 2026-03-15`.
pub fn title(app: &App, columns: usize) -> Line<'static> {
    let (start, length) = app.archive_cell(columns);
    let range = match length {
        1 => start.strftime("%a %Y-%m-%d").to_string(),
        _ => format!("{start} – {}", start + (length - 1).days()),
    };
    let count = app
        .archive_rows(columns)
        .iter()
        .filter(|row| matches!(row, ArchiveRow::Task(..)))
        .count();
    let tasks = if count == 1 { "task" } else { "tasks" };
    Line::from(vec![
        " ".into(),
        range.bold(),
        format!(" · {count} {tasks} ").dim(),
    ])
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
