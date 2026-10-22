//! The heatmap: done tasks per day, a week per column, see [`heatmap::columns`]. Scrolls
//! sideways, the tasks out of view are counted below it.

use jiff::{ToSpan, civil::Date};
use ratatui::{
    style::{Style, Stylize},
    text::{Line, Span},
};

use crate::{
    heatmap,
    ui::{CURRENT_COLOR, DONE_COLOR, LINE_COLOR, rows::more, spans},
};

/// Room for the weekdays on the right, next to the day cells they belong to.
const WEEKDAYS: usize = 4;

/// A cell and the space after it.
const STEP: usize = 2;

/// The glyph per step from little to a lot, see [`heatmap::bounds`]. Only the shade changes:
/// fonts draw it brighter in order, while dim, light and bold greens often look the same.
fn steps() -> [Span<'static>; 4] {
    ["░", "▒", "▓", "█"].map(|shade| shade.fg(DONE_COLOR))
}

/// The heatmap as [`lines`] draws it.
pub struct Heatmap {
    pub lines: Vec<Line<'static>>,
    /// The line below the heatmap: the tasks done in the weeks out of view on each side.
    pub hints: Line<'static>,
    /// See [`crate::app::archive::ArchivePage::heatmap_offset`].
    pub offset: usize,
    /// The cursor's row when it is in the first column on screen, its `[` goes before it.
    pub first_column_cursor: Option<i64>,
}

/// The days of [`heatmap::columns`] that fit in `width`, scrolled from `offset` so the
/// `selected_day` is on screen: labels, 7 rows of cells with the selected one in
/// [`CURRENT_COLOR`] and the weekdays on the right. With the `cursor` there too, the selected
/// cell is between brackets. With a filter, the cells without `matches` are gray, the shades
/// stay those of all tasks, and the hints count only matches.
pub fn lines(
    days: &heatmap::Days,
    matches: Option<&heatmap::Days>,
    today: Date,
    width: usize,
    offset: usize,
    selected_day: Date,
    cursor: bool,
) -> Heatmap {
    let columns = heatmap::columns(days, today);
    let visible = width.saturating_sub(WEEKDAYS) / STEP;
    let selected = heatmap::find(&columns, selected_day);
    let first = heatmap::first_visible(columns.len(), visible, selected.map(|(c, _)| c), offset);
    let end = (first + visible).min(columns.len());
    let shown = &columns[first..end];
    // Relative to the first column on screen.
    let selected = selected.map(|(column, row)| (column as i64 - first as i64, row));
    let bracketed = selected.filter(|_| cursor);

    // Counted over all days, so the shades stay while scrolling.
    let steps = steps();
    let bounds = heatmap::bounds(days.values().copied(), steps.len());
    let row_width = shown.len() * STEP + WEEKDAYS;
    let mut lines = vec![labels(shown, row_width)];

    // Nothing for a side without weeks out of view.
    let counted = matches.unwrap_or(days);
    let out_of_view = |columns: &[heatmap::Column]| {
        let (first, last) = columns.first().zip(columns.last())?;
        let range = counted.range(first.start..last.start + heatmap::CELLS.days());
        Some(more(range.map(|(_, done)| done).sum()))
    };
    let left = out_of_view(&columns[..first]).map_or(String::new(), |more| format!("← {more}"));
    let right = out_of_view(&columns[end..]).map_or(String::new(), |more| format!("{more} →"));
    let fill = row_width.saturating_sub(left.chars().count() + right.chars().count());
    let hints = Line::from(format!("{left}{}{right}", " ".repeat(fill))).fg(LINE_COLOR);

    for row in 0..heatmap::CELLS {
        let mut spans: Vec<Span> = Vec::new();
        for (i, column) in shown.iter().enumerate() {
            let i = i as i64;
            let day = column.day(row);
            // The cursor between brackets in the gaps around it.
            let gap: Span = match bracketed {
                Some(at) if at == (i, row) => "]".into(),
                Some(at) if at == (i + 1, row) => "[".into(),
                _ => " ".into(),
            };
            if day > today {
                spans.extend([" ".into(), gap]);
                continue;
            }
            let count = |days: &heatmap::Days| days.get(&day).copied().unwrap_or(0);
            let shade = match heatmap::level(count(days), &bounds) {
                0 => "·".fg(LINE_COLOR),
                level if matches.is_some_and(|matches| count(matches) == 0) => {
                    steps[level - 1].clone().fg(LINE_COLOR)
                }
                level => steps[level - 1].clone(),
            };
            // The selected cell in the current page's color, it shows without the brackets too.
            let shade = match selected == Some((i, row)) {
                true => shade.fg(CURRENT_COLOR),
                false => shade,
            };
            spans.extend([shade, gap]);
        }
        // Every other weekday, named after the day of its cell.
        let weekday = match shown.last() {
            Some(last) if matches!(row, 0 | 2 | 4) => last.day(row).strftime("%a").to_string(),
            _ => String::new(),
        };
        spans.push(weekday.dim());
        lines.push(Line::from(spans));
    }
    Heatmap {
        lines,
        hints,
        offset: columns.len() - end,
        first_column_cursor: bracketed.filter(|(i, _)| *i == 0).map(|(_, row)| row),
    }
}

/// The row above the cells: the month and year of the first column, so it is clear where the
/// view is, then months where they start, a year instead of January. Only where there is room.
fn labels(columns: &[heatmap::Column], width: usize) -> Line<'static> {
    let mut row = vec![(' ', Style::new()); width];
    let label = Style::new().dim();
    for (i, column) in columns.iter().enumerate() {
        let start = column.start;
        // A month only where its first Monday is, a week of the last one would push it out.
        let starts = start.day() <= 7;
        let text = match i {
            0 => start.strftime("%b %Y").to_string(),
            _ if starts && start.month() == 1 => start.year().to_string(),
            _ if starts => start.strftime("%b").to_string(),
            _ => continue,
        };
        put(&mut row, i * STEP, &text, label);
    }
    Line::from(spans(&row))
}

/// Writes `text` into `row` at `x` if it is free there, with a space on both sides.
fn put(row: &mut [(char, Style)], x: usize, text: &str, style: Style) {
    let end = x + text.chars().count();
    let free = |at: usize| row.get(at).is_none_or(|(c, _)| *c == ' ');
    if end <= row.len() && (x..end).all(free) && (x == 0 || free(x - 1)) && free(end) {
        for (i, c) in text.chars().enumerate() {
            row[x + i] = (c, style);
        }
    }
}
