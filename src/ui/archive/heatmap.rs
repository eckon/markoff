//! The heatmap: done tasks per cell over time, see [`heatmap::columns`].

use jiff::civil::Date;
use ratatui::{
    style::{Style, Stylize},
    text::{Line, Span},
};

use crate::{
    heatmap,
    ui::{CURRENT_COLOR, DONE_COLOR, LINE_COLOR, spans},
};

/// Room for the weekdays on the right, next to the day cells they belong to.
const WEEKDAYS: usize = 4;

/// A cell and the space after it.
const STEP: usize = 2;

/// How many columns fit in `width`, next to the weekdays.
pub fn columns(width: usize) -> usize {
    width.saturating_sub(WEEKDAYS) / STEP
}

/// The glyph per step from little to a lot, see [`heatmap::bounds`]. Only the shade changes:
/// fonts draw it brighter in order, while dim, light and bold greens often look the same.
fn steps() -> [Span<'static>; 4] {
    ["░", "▒", "▓", "█"].map(|shade| shade.fg(DONE_COLOR))
}

/// The tasks done over time, back to the first archived day, see [`heatmap::columns`]: labels,
/// 7 rows of cells with the one of `selected_day` in [`CURRENT_COLOR`] and the weekdays on the
/// right where cells are days, and a divider through the gap where the cell size changes. With
/// the `cursor` there too, it is between brackets.
pub fn lines(
    days: &heatmap::Days,
    today: Date,
    width: usize,
    selected_day: Date,
    cursor: bool,
) -> Vec<Line<'static>> {
    let columns = heatmap::columns(days, today, columns(width));
    let selected_cell = heatmap::find(&columns, selected_day);
    let bracketed = selected_cell.filter(|_| cursor);
    let steps = steps();
    let bounds = heatmap::bounds(
        columns.iter().flat_map(|column| {
            (0..heatmap::CELLS).map(|row| heatmap::rate(days, column.cell(row)))
        }),
        steps.len(),
    );
    // Weekdays only line up for day cells, which are on the right.
    let day_cells = columns.last().is_some_and(|column| column.unit == 1);

    let zone_starts = zone_starts(&columns);
    let mut lines = vec![labels(&columns, &zone_starts)];
    for row in 0..heatmap::CELLS {
        let mut spans: Vec<Span> = Vec::new();
        for (i, column) in columns.iter().enumerate() {
            let cell = column.cell(row);
            // The cursor between brackets in the gaps around it, over a divider too.
            let gap: Span = match zone_starts.get(i + 1) {
                _ if bracketed == Some((i, row)) => "]".into(),
                _ if bracketed == Some((i + 1, row)) => "[".into(),
                Some(true) => "│".fg(LINE_COLOR),
                _ => " ".into(),
            };
            if cell.0 > today {
                spans.extend([" ".into(), gap]);
                continue;
            }
            let shade = match heatmap::level(heatmap::rate(days, cell), &bounds) {
                0 => "·".fg(LINE_COLOR),
                level => steps[level - 1].clone(),
            };
            // The selected cell in the current page's color, it shows without the brackets too.
            let shade = match selected_cell == Some((i, row)) {
                true => shade.fg(CURRENT_COLOR),
                false => shade,
            };
            spans.extend([shade, gap]);
        }
        // Every other weekday, named after the day of its cell.
        let weekday = match columns.last() {
            Some(last) if day_cells && matches!(row, 0 | 2 | 4) => {
                last.cell(row).0.strftime("%a").to_string()
            }
            _ => String::new(),
        };
        spans.push(weekday.dim());
        lines.push(Line::from(spans));
    }
    lines
}

/// What one cell covers, the name of a zone of columns.
fn zone(unit: i64) -> &'static str {
    match unit {
        1 => "daily",
        2..=7 => "weekly",
        8..=31 => "monthly",
        _ => "yearly",
    }
}

/// Per column whether it starts another zone than the one before, a divider goes before it.
fn zone_starts(columns: &[heatmap::Column]) -> Vec<bool> {
    let mut previous = None;
    columns
        .iter()
        .map(|column| {
            let starts = previous.is_some_and(|previous| previous != zone(column.unit));
            previous = Some(zone(column.unit));
            starts
        })
        .collect()
}

/// The row above the cells: the dividers, each zone's name after it, and where there is room
/// months where cells are days and years elsewhere, at the column they start in.
fn labels(columns: &[heatmap::Column], zone_starts: &[bool]) -> Line<'static> {
    // Dates may run into the weekdays on the right.
    let mut row = vec![(' ', Style::new()); columns.len() * STEP + WEEKDAYS];
    for (i, _) in zone_starts
        .iter()
        .enumerate()
        .filter(|(_, starts)| **starts)
    {
        row[i * STEP - 1] = ('│', Style::new().fg(LINE_COLOR));
    }
    let name = Style::new().dim().italic();
    for (i, column) in columns.iter().enumerate() {
        if i == 0 || zone_starts[i] {
            put(&mut row, i * STEP, zone(column.unit), name);
        }
    }
    let mut previous = None;
    for (i, column) in columns.iter().enumerate() {
        let label = match column.unit {
            1 => column.start.strftime("%b").to_string(),
            _ => column.start.year().to_string(),
        };
        // A month only where it starts, a week of the last one would push it out.
        let starts = column.unit > 1 || column.start.day() <= 7;
        if previous.as_ref() != Some(&label) && starts {
            put(&mut row, i * STEP, &label, Style::new().dim());
        }
        previous = Some(label);
    }
    Line::from(spans(&row))
}

/// Writes `text` into `row` at `x` if it is free there, with a space or a divider on both sides.
fn put(row: &mut [(char, Style)], x: usize, text: &str, style: Style) {
    let end = x + text.chars().count();
    let free = |at: usize| row.get(at).is_none_or(|(c, _)| *c == ' ');
    let edge = |at: usize| row.get(at).is_none_or(|(c, _)| *c == ' ' || *c == '│');
    if end <= row.len() && (x..end).all(free) && (x == 0 || edge(x - 1)) && edge(end) {
        for (i, c) in text.chars().enumerate() {
            row[x + i] = (c, style);
        }
    }
}
