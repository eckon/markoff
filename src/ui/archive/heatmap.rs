//! The heatmap: done tasks per cell over time, see [`history::columns`].

use jiff::civil::Date;
use ratatui::{
    style::{Style, Stylize},
    text::{Line, Span},
};

use crate::{
    history,
    ui::{DONE_COLOR, HEADER_COLOR, LINE_COLOR, spans},
};

/// Room for the weekdays on the right, next to the day cells they belong to.
const WEEKDAYS: usize = 4;

/// A cell and the space after it.
const STEP: usize = 2;

/// How many columns fit in `width`, next to the weekdays.
pub fn columns(width: usize) -> usize {
    width.saturating_sub(WEEKDAYS) / STEP
}

/// The glyph per step from little to a lot, see [`history::bounds`]. Only the shade changes:
/// fonts draw it brighter in order, while dim, light and bold greens often look the same.
fn steps() -> [Span<'static>; 4] {
    ["░", "▒", "▓", "█"].map(|shade| shade.fg(DONE_COLOR))
}

/// The tasks done over time, back to the first archived day, see [`history::columns`]: labels,
/// 7 rows of cells with today in blue and the weekdays on the right where cells are days, and
/// a divider through the gap where the cell size changes.
pub fn lines(
    days: &history::Days,
    width: usize,
    selected: Date,
    focused: bool,
) -> Vec<Line<'static>> {
    let today = jiff::Zoned::now().date();
    let columns = history::columns(days, today, columns(width));
    let selected = history::find(&columns, selected);
    let steps = steps();
    let bounds = history::bounds(
        columns.iter().flat_map(|column| {
            (0..history::CELLS).map(|row| history::rate(days, column.cell(row)))
        }),
        steps.len(),
    );
    // Weekdays only line up for day cells, which are on the right.
    let day_cells = columns.last().is_some_and(|column| column.unit == 1);

    let zone_starts = zone_starts(&columns);
    let mut lines = vec![labels(&columns, &zone_starts)];
    for row in 0..history::CELLS {
        let mut spans: Vec<Span> = Vec::new();
        for (i, column) in columns.iter().enumerate() {
            let cell = column.cell(row);
            let gap: Span = match zone_starts.get(i + 1) {
                Some(true) => "│".fg(LINE_COLOR),
                _ => " ".into(),
            };
            if cell.0 > today {
                spans.extend([" ".into(), gap]);
                continue;
            }
            let shade = match history::level(history::rate(days, cell), &bounds) {
                0 => "·".fg(LINE_COLOR),
                level => steps[level - 1].clone(),
            };
            // Today in the header's blue, the cursor reversed, dim while the list has the keys.
            let shade = if cell.0 == today {
                shade.fg(HEADER_COLOR)
            } else {
                shade
            };
            let shade = match (selected == Some((i, row)), focused) {
                (true, true) => shade.reversed(),
                (true, false) => shade.reversed().dim(),
                (false, _) => shade,
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
fn zone_starts(columns: &[history::Column]) -> Vec<bool> {
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
fn labels(columns: &[history::Column], zone_starts: &[bool]) -> Line<'static> {
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
