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
    /// All of the line after the gutter: a section rule. A blank line goes
    /// before it, except at the top.
    Header(Vec<Span<'static>>),
    /// A task's title, wrapped to as many lines as it needs. What goes on the right edge of
    /// its first line, like the day a task was done. Whether it repeats, its bullet is `⟳` then.
    Task(Vec<Span<'static>>, Option<Span<'static>>, bool),
}

/// Space on the right so rules stop before the preview divider.
pub fn block<'a>() -> Block<'a> {
    Block::new().padding(Padding::new(1, 2, 0, 0))
}

/// The rows with the cursor on row `cursor`, `None` while the list does not have the keys. Then
/// the `notes` dimmed, a blank line before each unless it is at the top. Starts from `offset`,
/// returns the new one.
pub fn render(
    frame: &mut Frame,
    area: Rect,
    rows: Vec<Row>,
    (cursor, offset): (Option<usize>, usize),
    notes: &[&str],
) -> usize {
    let block = block();
    let width = block.inner(area).width as usize;
    let mut items: Vec<ListItem> = Vec::new();
    let mut selected = 0;
    for (i, row) in rows.into_iter().enumerate() {
        let is_cursor = cursor == Some(i);
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
        let (title, right, repeats) = match row.kind {
            Kind::Header(spans) => {
                let mut line = vec![gutter()];
                line.extend(spans);
                items.push(ListItem::new(Line::from(line)));
                continue;
            }
            Kind::Task(title, right, repeats) => (title, right, repeats),
        };
        // Gutter and indent before the title, the bullet below the key of the section.
        let prefix = |first: bool| -> Vec<Span<'static>> {
            let bullet = match (first, repeats) {
                (true, true) => "⟳ ".fg(row.color),
                (true, false) => "• ".fg(row.color),
                (false, _) => "  ".into(),
            };
            vec![
                gutter(),
                " ".repeat(KEY_OPEN.chars().count()).into(),
                bullet,
            ]
        };
        let right_width = right.as_ref().map_or(0, |right| right.width() + 1);
        let title_width = width.saturating_sub(Line::from(prefix(true)).width() + right_width);
        // Every title wrapped, not only the cursor's, so moving never shifts the rows below.
        let lines: Vec<Line> = wrap(title, title_width)
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
/// Every header in a list, so they all align: without a key (sections past nine) the lead is a
/// plain line as long.
pub fn section_rule(
    key: usize,
    color: Color,
    name: &str,
    count: usize,
    width: usize,
) -> Vec<Span<'static>> {
    // The key that jumps here replaces the leading `── ` of the rule, the key right above the
    // bullets. Without a key the rule is as long, the names align.
    let keyed = |key: char| {
        vec![
            KEY_OPEN.fg(LINE_COLOR),
            key.to_string().bold(),
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
    rule(
        Some(lead),
        name.to_string().fg(color).bold(),
        vec![count.to_string().dim()],
        width,
    )
}

/// Spans split into single styled chars, so they can be cut anywhere.
fn chars(spans: Vec<Span<'static>>) -> Vec<(char, Style)> {
    spans
        .iter()
        .flat_map(|span| span.content.chars().map(|c| (c, span.style)))
        .collect()
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

#[cfg(test)]
mod tests {
    use ratatui::{Terminal, backend::TestBackend, style::Color};

    use super::*;

    /// The rows in a list 40 wide, as text.
    fn draw(rows: Vec<Row>, cursor: usize, notes: &[&str]) -> Vec<String> {
        let mut terminal = Terminal::new(TestBackend::new(40, 8)).unwrap();
        terminal
            .draw(|frame| {
                render(frame, frame.area(), rows, (Some(cursor), 0), notes);
            })
            .unwrap();
        let buffer = terminal.backend().buffer();
        buffer
            .content()
            .chunks(buffer.area.width as usize)
            .map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>())
            .collect()
    }

    fn header(name: &str) -> Row {
        let rule = section_rule(0, Color::Blue, name, 1, 36);
        Row {
            color: Color::Blue,
            kind: Kind::Header(rule),
        }
    }

    fn task(title: &str, right: Option<&str>) -> Row {
        Row {
            color: Color::Blue,
            kind: Kind::Task(
                vec![title.to_string().into()],
                right.map(|r| r.to_string().into()),
                false,
            ),
        }
    }

    #[test]
    fn sections_with_a_blank_line_between_and_the_cursor_in_the_gutter() {
        let rows = vec![
            header("work"),
            task("a", None),
            header("home"),
            task("b", None),
        ];
        let lines = draw(rows, 3, &[]);
        // The padding, then the gutter.
        assert!(lines[0].starts_with("  ──[1]─ work ─"), "{lines:#?}");
        assert!(lines[0].trim_end().ends_with("─ 1"), "{lines:#?}");
        assert_eq!(lines[1].trim_end(), "     • a");
        assert_eq!(lines[2].trim(), "", "blank before the next section");
        assert_eq!(lines[4].trim_end(), " ▌   • b");
    }

    #[test]
    fn right_text_on_the_edge_of_the_first_line() {
        let long = "a title that is far too long for one line";
        let rows = vec![task(long, Some("today")), task(long, Some("today"))];
        let lines = draw(rows, 1, &[]);
        // 40 wide, the padding leaves 37: the title wraps a space before `today`, which stays on
        // its first line. With the cursor or without.
        assert_eq!(lines[0], "     • a title that is far too   today  ");
        assert_eq!(lines[1].trim_end(), "       long for one line");
        assert_eq!(lines[2], " ▌   • a title that is far too   today  ");
        assert_eq!(lines[3].trim_end(), " ▌     long for one line");
    }

    #[test]
    fn notes_after_the_rows_dimmed() {
        let lines = draw(Vec::new(), 0, &["nothing done"]);
        assert_eq!(lines[0].trim_end(), "  nothing done");

        let lines = draw(vec![task("a", None)], 0, &["no sections yet"]);
        assert_eq!(lines[1].trim(), "", "a blank line before a note");
        assert_eq!(lines[2].trim_end(), "  no sections yet");
    }
}
