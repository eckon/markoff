//! A list of sections and tasks as the todo list and the archive page show it: no box, sections
//! as rules, a bullet per task in the section color, the cursor a bar in the gutter. The pages
//! build the [`Row`]s, this lays them out.

use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, List, ListItem, ListState, Padding},
};

use super::{LINE_COLOR, rule, spans};
use crate::keymap;

/// One row of the list, the cursor moves over these.
pub struct Row {
    /// Of the cursor bar, and the bullet of a task.
    pub color: Color,
    pub kind: Kind,
}

pub enum Kind {
    /// All of the line after the gutter: a section rule or the daily header. A blank line goes
    /// before it, except at the top.
    Header(Vec<Span<'static>>),
    /// A task's title, cut to one line, wrapped at the cursor. What goes on the right edge of
    /// its first line, like the day a task was done.
    Task(Vec<Span<'static>>, Option<Span<'static>>),
}

/// Space on the right so rules stop before the preview divider.
pub fn block<'a>() -> Block<'a> {
    Block::new().padding(Padding::new(1, 2, 0, 0))
}

/// The rows with the cursor on row `cursor`, then the `notes` dimmed, a blank line before each
/// unless it is at the top. Starts from `offset`, returns the new one.
pub fn render(
    frame: &mut Frame,
    area: Rect,
    rows: Vec<Row>,
    [cursor, offset]: [usize; 2],
    notes: &[&str],
) -> usize {
    let block = block();
    let width = block.inner(area).width as usize;
    let mut items: Vec<ListItem> = Vec::new();
    let mut selected = 0;
    for (i, row) in rows.into_iter().enumerate() {
        let is_cursor = i == cursor;
        // Blank line before each section except the first. Not a row, the cursor skips it.
        if i > 0 && matches!(row.kind, Kind::Header(_)) {
            items.push(ListItem::new(""));
        }
        if is_cursor {
            selected = items.len();
        }
        // One column gutter for the cursor bar.
        let gutter = || -> Span<'static> {
            if is_cursor {
                "▌".fg(row.color)
            } else {
                " ".into()
            }
        };
        let (title, right) = match row.kind {
            Kind::Header(spans) => {
                let mut line = vec![gutter()];
                line.extend(spans);
                items.push(ListItem::new(Line::from(line)));
                continue;
            }
            Kind::Task(title, right) => (title, right),
        };
        // Gutter and indent before the title, the bullet below the key of the section.
        let prefix = |first: bool| -> Vec<Span<'static>> {
            let bullet = if first {
                "• ".fg(row.color)
            } else {
                "  ".into()
            };
            vec![
                gutter(),
                " ".repeat(KEY_OPEN.chars().count()).into(),
                bullet,
            ]
        };
        let right_width = right.as_ref().map_or(0, |right| right.width() + 1);
        let title_width = width.saturating_sub(Line::from(prefix(true)).width() + right_width);
        let title_lines = if is_cursor {
            wrap(title, title_width)
        } else {
            vec![truncate(title, title_width)]
        };
        let lines: Vec<Line> = title_lines
            .into_iter()
            .enumerate()
            .map(|(n, title)| {
                let mut spans = prefix(n == 0);
                spans.extend(title);
                if n == 0
                    && let Some(right) = right.clone()
                {
                    let used = Line::from(spans.clone()).width() + right.width();
                    spans.push(" ".repeat(width.saturating_sub(used)).into());
                    spans.push(right);
                }
                Line::from(spans)
            })
            .collect();
        items.push(ListItem::new(lines));
    }
    for note in notes {
        if !items.is_empty() {
            items.push(ListItem::new(""));
        }
        // Gutter column like the rows, so it lines up with the section rules.
        items.push(ListItem::new(format!(" {note}").dim()));
    }

    let list = List::new(items).block(block);
    // Starts from the last offset, ratatui only scrolls when the cursor would leave the
    // screen. A fresh offset would pin the cursor to the bottom edge while moving up.
    let mut state = ListState::default()
        .with_offset(offset)
        .with_selected(Some(selected));
    frame.render_stateful_widget(list, area, &mut state);
    state.offset()
}

/// Opens the section key in a section rule: `──[1]─ work`.
const KEY_OPEN: &str = "──[";

/// `──[1]─ work ─────── 2` with the key of the `key`th section, in `color`, filled to `width`.
pub fn section_rule(
    key: usize,
    color: Color,
    name: &str,
    count: usize,
    width: usize,
) -> Vec<Span<'static>> {
    // The key that jumps here replaces the leading `── ` of the rule, the key right above the
    // bullets. Dim like the count, so the name stays what stands out. Without a key the rule is
    // as long, the names align.
    let keyed = |key: char| {
        vec![
            KEY_OPEN.fg(LINE_COLOR),
            key.to_string().dim(),
            "]─ ".fg(LINE_COLOR),
        ]
    };
    let lead = match keymap::jump_key(key) {
        Some(key) => keyed(key),
        None => {
            let lead_width: usize = keyed(' ').iter().map(Span::width).sum();
            vec![format!("{} ", "─".repeat(lead_width - 1)).fg(LINE_COLOR)]
        }
    };
    let count = vec![count.to_string().dim()];
    rule(Some(lead), name, count, color, width)
}

/// Spans split into single styled chars, so they can be cut anywhere.
fn chars(spans: Vec<Span<'static>>) -> Vec<(char, Style)> {
    spans
        .iter()
        .flat_map(|span| span.content.chars().map(|c| (c, span.style)))
        .collect()
}

/// One line of `width`, ending in `…` when cut, so a long title doesn't just stop.
fn truncate(text: Vec<Span<'static>>, width: usize) -> Vec<Span<'static>> {
    let chars = chars(text);
    if chars.len() <= width {
        return spans(&chars);
    }
    let mut line = spans(&chars[..width.saturating_sub(1)]);
    line.push("…".dim());
    line
}

/// Lines of at most `width`, broken at spaces, words longer than a line are split.
fn wrap(text: Vec<Span<'static>>, width: usize) -> Vec<Vec<Span<'static>>> {
    let mut rest = chars(text);
    let mut lines = Vec::new();
    while rest.len() > width && width > 0 {
        let space = rest[..=width].iter().rposition(|(c, _)| *c == ' ');
        let (line, next) = match space {
            Some(at) if at > 0 => (&rest[..at], &rest[at + 1..]),
            _ => (&rest[..width], &rest[width..]),
        };
        lines.push(spans(line));
        rest = next.to_vec();
    }
    lines.push(spans(&rest));
    lines
}
