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
use unicode_width::UnicodeWidthChar;

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
/// returns the new one. What is scrolled out of `area` is counted on the lines around it, see
/// [`render_hidden`].
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
    // Which items are tasks, for the counts of the hidden ones.
    let mut tasks = Vec::new();
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
            Kind::Task(title, right, repeats) => {
                tasks.push(items.len());
                (title, right, repeats)
            }
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
        // Every title wraps, so moving the cursor never shifts the rows below.
        let lines: Vec<Line> = wrap(title, title_width)
            .into_iter()
            .enumerate()
            .map(|(n, title)| {
                let mut spans = prefix(n == 0);
                spans.extend(title);
                if n == 0
                    && let Some(right) = right.clone()
                {
                    // Dots lead to it, it is far from the title on a wide screen.
                    let used = Line::from(spans.clone()).width() + right.width();
                    let fill = width.saturating_sub(used);
                    spans.push(match fill {
                        0..3 => " ".repeat(fill).into(),
                        _ => format!(" {} ", "·".repeat(fill - 2)).fg(LINE_COLOR),
                    });
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

    let heights: Vec<usize> = items.iter().map(ListItem::height).collect();
    let list = List::new(items).block(block);
    // Starts from the last offset, ratatui only scrolls when the cursor would leave the
    // screen. A fresh offset would pin the cursor to the bottom edge while moving up.
    let mut state = ListState::default()
        .with_offset(offset)
        .with_selected(Some(selected));
    frame.render_stateful_widget(list, area, &mut state);
    render_hidden(frame, area, &heights, &tasks, state.offset());
    state.offset()
}

/// `↑ 4 more` on the line above `area` if items from `offset` on do not start at the top, `↓ 12
/// more` on the line below if they do not all fit: the tasks out of sight, those cut off at the
/// bottom too. Without a hidden task only `↑ more`. Both lists have a blank line there, between
/// them and the header or the status line.
fn render_hidden(frame: &mut Frame, area: Rect, heights: &[usize], tasks: &[usize], offset: usize) {
    let mut end = offset;
    let mut used = 0;
    while end < heights.len() && used + heights[end] <= area.height as usize {
        used += heights[end];
        end += 1;
    }
    let x = block().inner(area).x;
    let screen = frame.area();
    let hints = [
        (area.y.checked_sub(1), "↑", 0..offset),
        (Some(area.bottom()), "↓", end..heights.len()),
    ];
    for (y, arrow, hidden) in hints {
        let Some(y) = y.filter(|&y| !hidden.is_empty() && y >= screen.y && y < screen.bottom())
        else {
            continue;
        };
        let text = match tasks.iter().filter(|i| hidden.contains(i)).count() {
            0 => format!("{arrow} more"),
            count => format!("{arrow} {count} more"),
        };
        frame
            .buffer_mut()
            .set_span(x, y, &text.fg(LINE_COLOR), area.width);
    }
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

/// Spans split into single styled chars, so a line can break anywhere.
fn chars(spans: Vec<Span<'static>>) -> Vec<(char, Style)> {
    spans
        .iter()
        .flat_map(|span| span.content.chars().map(|c| (c, span.style)))
        .collect()
}

/// Lines of at most `width` cells, broken at spaces, words longer than a line are split. Wide
/// chars (`✅`, CJK) take two cells.
fn wrap(text: Vec<Span<'static>>, width: usize) -> Vec<Vec<Span<'static>>> {
    let mut rest = chars(text);
    let mut lines = Vec::new();
    loop {
        let mut used = 0;
        let fits = (rest.iter())
            .take_while(|(c, _)| {
                used += c.width().unwrap_or(0);
                used <= width
            })
            .count();
        if fits == rest.len() || width == 0 {
            break;
        }
        // The space right after the line is dropped too, so it may break there.
        let space = rest[..=fits].iter().rposition(|(c, _)| *c == ' ');
        // At least one char, even one wider than the line, or this never ends.
        let split = fits.max(1);
        let (line, next) = match space {
            Some(at) if at > 0 => (&rest[..at], &rest[at + 1..]),
            _ => (&rest[..split], &rest[split..]),
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
        let rows = vec![
            task(long, Some("today")),
            task(long, Some("today")),
            task("short", Some("today")),
        ];
        let lines = draw(rows, 1, &[]);
        // 40 wide, the padding leaves 37: the title wraps a space before `today`, which stays on
        // its first line. With the cursor or without.
        assert_eq!(lines[0], "     • a title that is far too · today  ");
        assert_eq!(lines[1].trim_end(), "       long for one line");
        assert_eq!(lines[2], " ▌   • a title that is far too · today  ");
        assert_eq!(lines[3].trim_end(), " ▌     long for one line");
        // Dots lead from a short title to it.
        assert_eq!(lines[4], "     • short ··················· today  ");
    }

    #[test]
    fn wide_chars_take_two_cells() {
        let lines = draw(vec![task(&"✅".repeat(13), Some("today"))], 1, &[]);
        // 12 fill the 24 cells left for the title, the gap to `today` stays. The test backend
        // shows the second cell of each as a space.
        assert_eq!(lines[0], format!("     • {}  today  ", "✅ ".repeat(12)));
        assert_eq!(lines[1].trim_end(), "       ✅");
    }

    #[test]
    fn hidden_tasks_counted_around_the_list() {
        let rows = || {
            let mut rows = vec![header("work")];
            rows.extend((0..6).map(|i| task(&format!("task {i}"), None)));
            rows
        };
        let draw = |cursor: usize| -> Vec<String> {
            let mut terminal = Terminal::new(TestBackend::new(30, 6)).unwrap();
            terminal
                .draw(|frame| {
                    // A blank line above and below, like on both pages.
                    let area = Rect::new(0, 1, 30, 4);
                    render(frame, area, rows(), (Some(cursor), 0), &[]);
                })
                .unwrap();
            let buffer = terminal.backend().buffer();
            (buffer.content().chunks(30))
                .map(|row| row.iter().map(|cell| cell.symbol()).collect())
                .collect()
        };

        // At the top only below: 3 of 6 tasks fit.
        let lines = draw(0);
        assert_eq!(lines[0].trim(), "", "{lines:#?}");
        assert_eq!(lines[5].trim(), "↓ 3 more", "{lines:#?}");

        // At the bottom the header and two tasks are above.
        let lines = draw(6);
        assert_eq!(lines[0].trim(), "↑ 2 more", "{lines:#?}");
        assert_eq!(lines[5].trim(), "", "{lines:#?}");

        // Only the header above: no task to count.
        let lines = draw(4);
        assert_eq!(lines[0].trim(), "↑ more", "{lines:#?}");
        assert_eq!(lines[5].trim(), "↓ 2 more", "{lines:#?}");
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
