//! Rendering: header, list, preview, status line and the help. Reads the app, never changes it.

use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style, Stylize},
    text::{Line, Span, Text},
    widgets::{
        Block, BorderType, Borders, Clear, List, ListItem, ListState, Padding, Paragraph, Wrap,
    },
};

use crate::{
    app::{self, App, Row, Status},
    filter, history,
    keymap::{Action, BINDINGS, Binding, FILTER_INPUT_HINTS},
};

/// Draws the whole screen from the current state. Rendering never mutates the app, it
/// returns the list's scroll offset for the caller to keep in [`App::list_offset`].
pub fn render(app: &App, frame: &mut Frame) -> usize {
    // The header gets a blank line below, like the one above every section.
    let [header_area, gap_area, body_area, status_area] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(0),
        Constraint::Length(1),
    ])
    .areas(frame.area());
    // The heatmap with a blank line above, only once something is archived and only if the
    // list keeps enough room: the tasks come first.
    let heatmap_block = list_block().padding(Padding::new(1, 2, 1, 0));
    let heatmap = match app.days.is_empty() {
        true => Vec::new(),
        false => heatmap_lines(&app.days, heatmap_block.inner(body_area).width as usize),
    };
    let heatmap_height = heatmap.len() as u16 + 1;
    let (main_area, heatmap_area) =
        if !heatmap.is_empty() && body_area.height >= MIN_LIST_HEIGHT + heatmap_height {
            let [main, heatmap] =
                Layout::vertical([Constraint::Min(0), Constraint::Length(heatmap_height)])
                    .areas(body_area);
            (main, Some(heatmap))
        } else {
            (body_area, None)
        };
    if let Some(area) = heatmap_area {
        frame.render_widget(Paragraph::new(heatmap).block(heatmap_block), area);
    }
    render_header(app, frame, header_area);
    let [list_area, preview_area] =
        Layout::horizontal([Constraint::Percentage(40), Constraint::Percentage(60)])
            .areas(main_area);
    // Rules keep the split width even when the list gets the whole screen,
    // so they don't jump while moving between tasks with and without a body.
    let rule_width = list_block().inner(list_area).width as usize;

    // The preview only shows up for a task with a body, otherwise the list takes all the width.
    let body = match app.selected() {
        Some(Row::Task(s, t)) => Some(app.sections[s].tasks[t].body.as_str()),
        Some(Row::DailyTask(i)) => app.daily.as_ref().map(|daily| daily[i].body.as_str()),
        _ => None,
    };
    let offset = match body {
        Some(body) if !body.trim().is_empty() => {
            render_preview(app, frame, preview_area, body);
            connect_divider(frame, preview_area.x, header_area.y, gap_area.y);
            render_list(app, frame, list_area, rule_width)
        }
        _ => render_list(app, frame, main_area, rule_width),
    };
    render_status(app, frame, status_area);
    if app.show_help {
        render_help(frame);
    }
    offset
}

/// A panel on the right listing [`BINDINGS`], so it always matches the real keys. Groups as
/// rules like the sections in the list, dots lead from the keys to what they do. The rest is
/// dimmed behind it, the status line stays as it is.
fn render_help(frame: &mut Frame) {
    let label = |binding: &Binding| binding.labels().join(" / ");
    let label_width = BINDINGS
        .iter()
        .map(|binding| label(binding).chars().count())
        .max()
        .unwrap_or(0);
    // Rules as wide as the widest line: keys, the dots with a space on each side, the text.
    let help_width = BINDINGS
        .iter()
        .map(|binding| binding.help.chars().count())
        .max()
        .unwrap_or(0);
    let rule_width = label_width + 4 + help_width;

    let mut lines = vec![Line::from("help".bold())];
    let mut groups = 0;
    for (i, binding) in BINDINGS.iter().enumerate() {
        if i == 0 || BINDINGS[i - 1].group != binding.group {
            lines.push(Line::default());
            lines.push(Line::from(rule(
                binding.group,
                None,
                section_color(groups),
                rule_width,
            )));
            groups += 1;
        }
        // Only the keys are bold, so the `/` between them is not taken for the filter key.
        let mut line = Vec::new();
        for key in binding.labels() {
            if !line.is_empty() {
                line.push(" / ".dim());
            }
            line.push(key.bold());
        }
        let dots = "·".repeat(label_width + 2 - label(binding).chars().count());
        line.push(format!(" {dots} ").fg(LINE_COLOR));
        line.push(binding.help.into());
        lines.push(Line::from(line));
    }

    let area = frame.area();
    let height = area.height.saturating_sub(1);
    let behind = Rect { height, ..area };
    frame.buffer_mut().set_style(behind, Modifier::DIM);
    // Border and padding take 4 columns.
    let width = (lines.iter().map(Line::width).max().unwrap_or(0) as u16 + 4).min(area.width);
    let panel = Rect::new(area.right() - width, area.y, width, height);
    frame.render_widget(Clear, panel);
    frame.render_widget(
        Paragraph::new(lines).block(
            Block::new()
                .borders(Borders::LEFT)
                .border_type(BorderType::Double)
                .border_style(LINE_COLOR)
                .padding(Padding::new(2, 1, 1, 0)),
        ),
        panel,
    );
}

/// Lines the list keeps before the heatmap below it is left out.
const MIN_LIST_HEIGHT: u16 = 10;

/// Room for the weekdays on the right, next to the day cells they belong to.
const WEEKDAYS: usize = 4;

/// A cell and the space after it.
const STEP: usize = 2;

/// The glyph per step from little to a lot, see [`history::bounds`]. Only the shade changes:
/// fonts draw it brighter in order, while dim, light and bold greens often look the same.
fn steps() -> [Span<'static>; 4] {
    ["░", "▒", "▓", "█"].map(|shade| shade.fg(DONE_COLOR))
}

/// The tasks done over time, back to the first archived day, see [`history::columns`]: labels,
/// 7 rows of cells with today in brackets and the weekdays on the right where cells are days,
/// and a divider through the gap where the cell size changes.
fn heatmap_lines(days: &history::Days, width: usize) -> Vec<Line<'static>> {
    let today = jiff::Zoned::now().date();
    let room = width.saturating_sub(WEEKDAYS);
    let columns = history::columns(days, today, room / STEP);
    let steps = steps();
    let bounds = history::bounds(
        columns
            .iter()
            .flat_map(|column| (0..7).map(|row| history::rate(days, column.cell(row)))),
        steps.len(),
    );
    // Weekdays only line up for day cells, which are on the right.
    let day_cells = columns.last().is_some_and(|column| column.unit == 1);

    let zone_starts = zone_starts(&columns);
    let mut lines = vec![labels(&columns, &zone_starts)];
    for row in 0..7 {
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
            // The brackets go into the gaps around it.
            if cell.0 == today && spans.last().is_some_and(|gap| gap.content == " ") {
                spans.pop();
                spans.extend(["[".bold(), shade, "]".bold()]);
            } else {
                spans.extend([shade, gap]);
            }
        }
        let weekday = match (day_cells, row) {
            (true, 0) => "Mon",
            (true, 2) => "Wed",
            (true, 4) => "Fri",
            _ => "",
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

/// Joins the preview divider at column `x` to the header: `┬` in the header rule, `│` in the
/// blank line below. Only where the rule is a line, never over the date or the counts.
fn connect_divider(frame: &mut Frame, x: u16, header_y: u16, gap_y: u16) {
    let buffer = frame.buffer_mut();
    if buffer[(x, header_y)].symbol() == "─" {
        buffer[(x, header_y)].set_symbol("┬");
    }
    buffer[(x, gap_y)].set_symbol("│").set_fg(LINE_COLOR);
}

/// `── Sat 17 Oct ──────── 4 open · ✓ 2 done` over the whole width: today, the tasks in all
/// sections and those archived today, so done tasks don't just vanish.
fn render_header(app: &App, frame: &mut Frame, area: Rect) {
    let date = jiff::Zoned::now().strftime("%a %d %b").to_string();
    let open: usize = app.sections.iter().map(|section| section.tasks.len()).sum();
    let done = app.done_today;

    let mut spans = rule(
        &date,
        Some(format!("{open} open · ✓ {done} done")),
        HEADER_COLOR,
        area.width as usize,
    );
    // Same text as the count `rule` made, only the check in green.
    spans.pop();
    spans.push(format!(" {open} open · ").dim());
    spans.push("✓".fg(DONE_COLOR));
    spans.push(format!(" {done} done").dim());
    frame.render_widget(Line::from(spans), area);
}

/// Left the filter input or what just happened, right the key hints.
fn render_status(app: &App, frame: &mut Frame, area: Rect) {
    let left = if app.filter_input {
        let filter = format!("/{}", app.filter);
        let end = area.x + Line::from(filter.as_str()).width() as u16;
        frame.set_cursor_position((end.min(area.right().saturating_sub(1)), area.y));
        Line::from(filter)
    } else {
        status_text(app)
    };

    // Hints give way from the end, the left side is what just happened.
    let mut hints = hints(app);
    let width = |hints: &[(String, &str)]| {
        let keys_and_texts: usize = hints
            .iter()
            .map(|(key, text)| key.chars().count() + 1 + text.chars().count())
            .sum();
        keys_and_texts + 2 * hints.len().saturating_sub(1)
    };
    while !hints.is_empty() && left.width() + 2 + width(&hints) > area.width as usize {
        hints.pop();
    }
    let mut spans = Vec::new();
    for (i, (key, text)) in hints.into_iter().enumerate() {
        if i > 0 {
            spans.push("  ".into());
        }
        spans.push(key.bold());
        spans.push(format!(" {text}").dim());
    }

    frame.render_widget(left, area);
    frame.render_widget(Line::from(spans).right_aligned(), area);
}

/// Key hints per mode: how to get out of the filter, and where to find the rest. Keys from
/// [`BINDINGS`], so they always match. Own texts, the help's would not fit every mode.
fn hints(app: &App) -> Vec<(String, &'static str)> {
    if app.filter_input {
        return FILTER_INPUT_HINTS
            .iter()
            .map(|(key, text)| (key.to_string(), *text))
            .collect();
    }
    let help = (Action::Help, "toggle help");
    let hints = if app.filter.is_empty() || app.show_help {
        vec![help]
    } else {
        vec![(Action::FilterClear, "clear filter"), help]
    };
    hints
        .into_iter()
        .filter_map(|(action, text)| {
            BINDINGS
                .iter()
                .find(|b| b.keys.iter().any(|(_, a)| *a == action))
                .map(|b| (b.labels().join(" / "), text))
        })
        .collect()
}

/// The left side of the status line when the filter input is closed.
fn status_text(app: &App) -> Line<'_> {
    match (&app.status, app.keymap.pending()) {
        (Some(Status::Error(message)), _) => Line::from(message.as_str().red()),
        (Some(Status::Info(message)), _) => Line::from(message.as_str()),
        (None, Some(key)) => Line::from(key.to_string()),
        (None, None) if !app.filter.is_empty() => {
            let count = app
                .rows()
                .iter()
                .filter(|row| matches!(row, Row::Task(..)))
                .count();
            let tasks = if count == 1 { "task" } else { "tasks" };
            Line::from(vec![
                "filter: ".dim(),
                app.filter.as_str().bold(),
                format!(" · {count} {tasks}").dim(),
            ])
        }
        (None, None) => Line::default(),
    }
}

/// Colors of the sections in list order, repeating. Only the 16 terminal
/// colors, so they follow the terminal theme.
const SECTION_COLORS: [Color; 4] = [Color::Blue, Color::Magenta, Color::Green, Color::Yellow];

/// The daily checklist, outside the section colors so those stay as they are.
const DAILY_COLOR: Color = Color::Cyan;

/// The date in the header.
const HEADER_COLOR: Color = Color::Blue;

/// The check for tasks done today.
const DONE_COLOR: Color = Color::Green;

/// Rules and the preview divider.
const LINE_COLOR: Color = Color::DarkGray;

fn section_color(index: usize) -> Color {
    SECTION_COLORS[index % SECTION_COLORS.len()]
}

/// No box around the list. Sections are rules, tasks get a bullet in the section color,
/// the cursor is a bar in the gutter. Returns the scroll offset.
fn render_list(app: &App, frame: &mut Frame, area: Rect, width: usize) -> usize {
    let block = list_block();
    let text_width = block.inner(area).width as usize;

    let mut items: Vec<ListItem> = Vec::new();
    let mut selected = 0;
    let rows = app.rows();
    let today = app::today();
    for (i, &row) in rows.iter().enumerate() {
        let is_cursor = i == app.cursor;
        let color = match row {
            Row::Section(s) | Row::Task(s, _) => section_color(s),
            Row::Daily | Row::DailyTask(_) => DAILY_COLOR,
        };

        // Blank line before each section except the first. Not a row, the cursor skips it.
        if i > 0 && matches!(row, Row::Section(_) | Row::Daily) {
            items.push(ListItem::new(""));
        }
        if is_cursor {
            selected = items.len();
        }

        // One column gutter for the cursor bar.
        let gutter = || -> Span<'static> {
            if is_cursor {
                "▌".fg(color)
            } else {
                " ".into()
            }
        };
        let task_style = if is_cursor {
            Style::new().fg(color)
        } else {
            Style::new()
        };
        let title = match row {
            Row::Section(s) => {
                let section = &app.sections[s];
                // Only the shown tasks, so with a filter it says how many match.
                let count = rows
                    .iter()
                    .filter(|row| matches!(row, Row::Task(task_s, _) if *task_s == s))
                    .count();
                // `1`-`9` jump here. The key replaces the leading `── ` of the rule, 4 columns
                // wider, the digit right above the bullets. Dim like the count, so the name
                // stays what stands out. Without a key the rule is as long, the names align.
                let lead = if s < 9 {
                    vec![
                        "──[".fg(LINE_COLOR),
                        (s + 1).to_string().dim(),
                        "]─ ".fg(LINE_COLOR),
                    ]
                } else {
                    vec!["────── ".fg(LINE_COLOR)]
                };
                let mut spans = vec![gutter()];
                spans.extend(rule(
                    &section.name,
                    Some(count.to_string()),
                    color,
                    width.saturating_sub(5),
                ));
                spans.splice(1..2, lead);
                items.push(ListItem::new(Line::from(spans)));
                continue;
            }
            Row::Daily => {
                let daily = app.daily.as_deref().unwrap_or_default();
                let open = daily.iter().filter(|t| !t.done_on(&today)).count();
                let count = format!("{open}/{}", daily.len());
                let mut spans = vec![gutter()];
                spans.extend(rule("daily", Some(count), color, width.saturating_sub(1)));
                items.push(ListItem::new(Line::from(spans)));
                continue;
            }
            Row::DailyTask(i) => {
                let task = &app.daily.as_deref().unwrap_or_default()[i];
                let mut style = task_style;
                if task.done_on(&today) {
                    style = style.dim().crossed_out();
                }
                vec![Span::styled(task.title.clone(), style)]
            }
            Row::Task(s, t) => {
                let title = app.sections[s].tasks[t].title();
                let mut style = task_style;
                // Listed for a match in the body only: weaker, `…` hints at the preview.
                let body_only = !filter::matches(&title, &app.filter);
                if body_only {
                    style = style.dim();
                }
                let mut spans = highlight(&title, &app.filter, style);
                if body_only {
                    spans.push(" …".dim());
                }
                spans
            }
        };

        // Gutter and `   • ` before the title, the bullet below the digit of the section key.
        let title_width = text_width.saturating_sub(6);
        let title_lines = if is_cursor {
            wrap(title, title_width)
        } else {
            vec![truncate(title, title_width)]
        };
        let lines: Vec<Line> = title_lines
            .into_iter()
            .enumerate()
            .map(|(n, title)| {
                let bullet = if n == 0 {
                    "• ".fg(color)
                } else {
                    "  ".into()
                };
                let mut spans = vec![gutter(), "   ".into(), bullet];
                spans.extend(title);
                Line::from(spans)
            })
            .collect();
        items.push(ListItem::new(lines));
    }
    if items.is_empty() && !app.filter.is_empty() {
        // Gutter column like the rows, so it lines up with the section rules.
        items.push(ListItem::new(" no matches".dim()));
    }
    // Tasks only live in sections, so without one there is nowhere to add a task.
    if app.sections.is_empty() {
        if !items.is_empty() {
            items.push(ListItem::new(""));
        }
        items.push(ListItem::new(" no sections yet, N creates one".dim()));
    }

    let list = List::new(items).block(block);
    // Starts from the last offset, ratatui only scrolls when the cursor would leave the
    // screen. A fresh offset would pin the cursor to the bottom edge while moving up.
    let mut state = ListState::default()
        .with_offset(app.list_offset)
        .with_selected(Some(selected));
    frame.render_stateful_widget(list, area, &mut state);
    state.offset()
}

/// Spans split into single styled chars, so they can be cut anywhere.
fn chars(spans: Vec<Span<'static>>) -> Vec<(char, Style)> {
    spans
        .iter()
        .flat_map(|span| span.content.chars().map(|c| (c, span.style)))
        .collect()
}

/// Styled chars joined back into spans, one per run of the same style.
fn spans(chars: &[(char, Style)]) -> Vec<Span<'static>> {
    chars
        .chunk_by(|a, b| a.1 == b.1)
        .map(|run| Span::styled(run.iter().map(|(c, _)| c).collect::<String>(), run[0].1))
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

/// Space on the right so rules stop before the preview divider.
fn list_block<'a>() -> Block<'a> {
    Block::new().padding(Padding::new(1, 2, 0, 0))
}

/// `── work ─────── 2`, filled to `width`. Section headers, the date header, help groups.
fn rule(title: &str, count: Option<String>, color: Color, width: usize) -> Vec<Span<'static>> {
    let count = count.map(|c| format!(" {c}")).unwrap_or_default();
    let used = 3 + title.chars().count() + 1 + count.chars().count();
    vec![
        "── ".fg(LINE_COLOR),
        title.to_string().fg(color).bold(),
        " ".into(),
        "─".repeat(width.saturating_sub(used)).fg(LINE_COLOR),
        count.dim(),
    ]
}

/// Task body as raw text, read only. No title, the cursor row in the list shows it in full.
fn render_preview(app: &App, frame: &mut Frame, area: Rect, body: &str) {
    let text = body_text(body, &app.filter);

    // No box, a divider on the left.
    let divider = Block::new()
        .borders(Borders::LEFT)
        .border_style(Style::new().fg(LINE_COLOR))
        .padding(Padding::horizontal(1));
    let inner = divider.inner(area);
    frame.render_widget(divider, area);
    frame.render_widget(Paragraph::new(text).wrap(Wrap { trim: false }), inner);
}

/// Markdown stays raw. Filter matches are highlighted, so a task listed for its body shows why.
fn body_text(body: &str, filter: &str) -> Text<'static> {
    body.lines()
        .map(|line| Line::from(highlight(line, filter, Style::new())))
        .collect()
}

/// Filter matches: yellow alone would vanish in the yellow section, bold keeps them visible.
const MATCH_STYLE: Style = Style::new().fg(Color::Yellow).add_modifier(Modifier::BOLD);

/// `text` in `style`, with the matches of `filter` in [`MATCH_STYLE`] on top.
fn highlight(text: &str, filter: &str, style: Style) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    let mut at = 0;
    for range in filter::find(text, filter) {
        spans.push(Span::styled(text[at..range.start].to_string(), style));
        spans.push(Span::styled(
            text[range.clone()].to_string(),
            style.patch(MATCH_STYLE),
        ));
        at = range.end;
    }
    spans.push(Span::styled(text[at..].to_string(), style));
    spans
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    use crate::{app::tests::sections, keymap::Action};
    use jiff::ToSpan;
    use ratatui::{Terminal, backend::TestBackend, style::Modifier};

    /// Renders the app and returns the screen as text, styles (bold, highlight) are ignored.
    fn screen(app: &App) -> String {
        let mut terminal = Terminal::new(TestBackend::new(50, 9)).unwrap();
        terminal
            .draw(|frame| {
                render(app, frame);
            })
            .unwrap();

        let buffer = terminal.backend().buffer();
        buffer
            .content()
            .chunks(buffer.area.width as usize)
            .map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Draws like the main loop: the scroll offset of one frame is where the next starts.
    fn draw(app: &mut App) -> Vec<String> {
        let mut offset = app.list_offset;
        let mut terminal = Terminal::new(TestBackend::new(50, 9)).unwrap();
        terminal.draw(|frame| offset = render(app, frame)).unwrap();
        app.list_offset = offset;
        screen(app).lines().map(str::to_string).collect()
    }

    #[test]
    fn list_scrolls_only_when_the_cursor_leaves_the_screen() {
        let titles: Vec<String> = (1..=20).map(|i| format!("Task {i}")).collect();
        let titles: Vec<&str> = titles.iter().map(String::as_str).collect();
        let mut app = App::new(PathBuf::new(), sections(&[("home", &titles)]));

        app.update(Action::Bottom);
        let lines = draw(&mut app);
        assert!(lines[7].contains("▌   • Task 20"), "{lines:#?}");

        // Moving up inside the screen moves the cursor, not the view.
        app.update(Action::Up);
        draw(&mut app);
        app.update(Action::Up);
        let lines = draw(&mut app);
        assert!(lines[7].contains("• Task 20"), "{lines:#?}");
        assert!(lines[5].contains("▌   • Task 18"), "{lines:#?}");
    }

    #[test]
    fn long_titles_cut_and_wrap_at_the_cursor() {
        let long = "Write the long report about every open question";
        let mut app = App::new(PathBuf::new(), sections(&[("home", &[long, long])]));

        app.update(Action::Down);
        let lines = draw(&mut app);
        // 50 wide: padding, gutter and `   • ` leave 41 for the title.
        assert_eq!(
            lines[3],
            " ▌   • Write the long report about every open     "
        );
        assert_eq!(lines[4].trim_end(), " ▌     question");
        assert_eq!(
            lines[5].trim_end(),
            "     • Write the long report about every open q…"
        );
    }

    #[test]
    fn renders_sections_and_tasks() {
        let app = App::new(
            PathBuf::new(),
            sections(&[("home", &["Fix typo"]), ("work", &["Review PR"])]),
        );
        let screen = screen(&app);
        for text in ["──[1]─ home", "• Fix typo", "──[2]─ work", "• Review PR"] {
            assert!(screen.contains(text), "missing {text:?} in\n{screen}");
        }
    }

    #[test]
    fn sections_past_nine_have_no_key() {
        let names: Vec<String> = (1..=10).map(|i| format!("s{i}")).collect();
        let spec: Vec<(&str, &[&str])> = names.iter().map(|n| (n.as_str(), &[][..])).collect();
        let mut app = App::new(PathBuf::new(), sections(&spec));
        app.update(Action::Bottom);
        let screen = screen(&app);
        // Same length as `──[9]─ `, so the names stay aligned.
        assert!(screen.contains("──[9]─ s9 "), "{screen}");
        assert!(screen.contains("────── s10 "), "{screen}");
    }

    #[test]
    fn preview_shows_task_body() {
        let mut app = App::new(PathBuf::new(), sections(&[("home", &["Fix typo"])]));
        app.sections[0].tasks[0].body = "line one\n## Notes\nline two".to_string();
        app.update(Action::Down);

        let screen = screen(&app);
        for text in ["line one", "## Notes", "line two"] {
            assert!(screen.contains(text), "missing {text:?} in\n{screen}");
        }
        // The title is only in the list, the preview starts with the body.
        assert_eq!(screen.matches("Fix typo").count(), 1, "{screen}");
    }

    #[test]
    fn preview_divider_joins_the_header() {
        let mut app = App::new(
            PathBuf::new(),
            sections(&[("home", &["Fix typo", "Empty"])]),
        );
        app.sections[0].tasks[0].body = "line one".to_string();
        let column = |line: &str, c: char| line.chars().position(|x| x == c);

        app.update(Action::Down);
        let lines: Vec<String> = screen(&app).lines().map(str::to_string).collect();
        let divider = column(&lines[2], '│').expect("preview divider");
        assert_eq!(column(&lines[0], '┬'), Some(divider), "{lines:#?}");
        assert_eq!(column(&lines[1], '│'), Some(divider), "{lines:#?}");

        // Without a preview the header is a plain line.
        app.update(Action::Down);
        let lines: Vec<String> = screen(&app).lines().map(str::to_string).collect();
        assert!(!lines[0].contains('┬'), "{lines:#?}");
        assert_eq!(lines[1].trim(), "");
    }

    #[test]
    fn status_line_shows_errors() {
        let mut app = App::new(PathBuf::new(), sections(&[("home", &[])]));
        app.update(Action::Undo);
        let screen = screen(&app);
        assert!(
            screen.contains("nothing to undo"),
            "missing status in\n{screen}"
        );
    }

    #[test]
    fn help_lists_bindings() {
        let mut app = App::new(PathBuf::new(), sections(&[("home", &[])]));
        app.update(Action::Help);
        let mut terminal = Terminal::new(TestBackend::new(60, 30)).unwrap();
        terminal
            .draw(|frame| {
                render(&app, frame);
            })
            .unwrap();
        let screen: String = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|c| c.symbol())
            .collect();
        for text in [
            "── move ──",
            "j / k ···· down / up",
            "o / O ···· new task below / above",
            "D ········ delete",
            "q / C-c ·· quit",
            "── other ──",
        ] {
            assert!(screen.contains(text), "missing {text:?}");
        }

        // Keys are bold, the `/` between them is dim. The filter key `/` is bold.
        let content = terminal.backend().buffer().content();
        let modifier = |text: &str, offset: usize| {
            let start = screen.find(text).unwrap();
            content[screen[..start + offset].chars().count()].modifier
        };
        assert_eq!(modifier("j / k ·", 0), Modifier::BOLD);
        assert_eq!(modifier("j / k ·", 2), Modifier::DIM);
        assert_eq!(modifier("/ ····", 0), Modifier::BOLD);

        // The list behind the panel is dimmed, the status line is not.
        assert!(modifier("home", 0).contains(Modifier::DIM));
        assert_eq!(modifier("? toggle help", 0), Modifier::BOLD);
    }

    #[test]
    fn filter_input_and_filtered_list() {
        let mut app = App::new(
            PathBuf::new(),
            sections(&[("home", &["Fix typo"]), ("work", &["Review"])]),
        );
        app.update(Action::Filter);
        for c in "rev".chars() {
            app.update(Action::FilterType(c));
        }
        let lines: Vec<String> = screen(&app).lines().map(str::to_string).collect();
        // The first visible section starts right below the header, without another blank line.
        assert!(lines[2].contains("work"), "{lines:#?}");
        assert!(lines[3].contains("Review"), "{lines:#?}");
        assert!(!lines.join("\n").contains("Fix typo"));
        assert!(lines[8].starts_with("/rev "), "input at the bottom");
        assert!(lines[8].ends_with("<CR> keep filter  Esc clear filter"));

        // Accepted, the filter stays visible in the status line, with its matches.
        app.update(Action::FilterAccept);
        let screen = screen(&app);
        let status = screen.lines().last().unwrap();
        assert!(status.starts_with("filter: rev · 1 task "), "{status:?}");
        // `? toggle help` gives way, 50 columns are too narrow for both hints.
        assert!(status.ends_with("Esc clear filter"), "{status:?}");
    }

    #[test]
    fn status_hints_give_way_to_messages() {
        let mut app = App::new(PathBuf::new(), sections(&[("home", &[])]));
        assert!(
            screen(&app)
                .lines()
                .last()
                .unwrap()
                .ends_with("? toggle help")
        );

        // 50 columns: no room for the hint next to this message.
        let message = "x".repeat(44);
        app.status = Some(Status::Info(message.clone()));
        assert_eq!(screen(&app).lines().last().unwrap().trim_end(), message);
    }

    #[test]
    fn header_shows_date_open_and_done() {
        let mut app = App::new(
            PathBuf::new(),
            sections(&[("home", &["Fix typo"]), ("work", &["Review", "Deploy"])]),
        );
        app.done_today = 2;
        let mut terminal = Terminal::new(TestBackend::new(50, 9)).unwrap();
        terminal
            .draw(|frame| {
                render(&app, frame);
            })
            .unwrap();
        let lines: Vec<String> = screen(&app).lines().map(str::to_string).collect();

        let date = jiff::Zoned::now().strftime("%a %d %b").to_string();
        assert!(lines[0].starts_with(&format!("── {date} ─")), "{lines:#?}");
        assert!(lines[0].ends_with("─ 3 open · ✓ 2 done"), "{lines:#?}");
        let check = lines[0].chars().position(|c| c == '✓').unwrap() as u16;
        assert_eq!(terminal.backend().buffer()[(check, 0)].fg, DONE_COLOR);

        // A blank line, then the sections.
        assert_eq!(lines[1].trim(), "");
        assert!(lines[2].contains("home"), "{lines:#?}");
    }

    #[test]
    fn filter_without_matches_says_so() {
        let mut app = App::new(PathBuf::new(), sections(&[("home", &["Fix typo"])]));
        assert!(!screen(&app).contains("no matches"), "not without a filter");

        app.update(Action::Filter);
        app.update(Action::FilterType('x'));
        app.update(Action::FilterType('q'));
        assert!(screen(&app).lines().nth(2).unwrap().contains("no matches"));
    }

    #[test]
    fn daily_shows_open_count_and_strikes_done() {
        let mut app = App::new(PathBuf::new(), sections(&[("home", &[])]));
        let today = app::today();
        app.daily = Some(crate::daily::parse(&format!(
            "## Jira [done {today}]\n## Menu [done 2000-01-01]\n\nhttps://menu\n"
        )));
        let mut terminal = Terminal::new(TestBackend::new(50, 9)).unwrap();
        terminal
            .draw(|frame| {
                render(&app, frame);
            })
            .unwrap();
        let buffer = terminal.backend().buffer();
        let lines: Vec<String> = screen(&app).lines().map(str::to_string).collect();

        // Right below the header, above the sections.
        assert!(
            lines[2].contains("daily") && lines[2].trim_end().ends_with(" 1/2"),
            "{lines:#?}"
        );
        assert!(lines[3].contains("• Jira"), "{lines:#?}");
        assert!(
            buffer[(7, 3)]
                .modifier
                .contains(Modifier::CROSSED_OUT | Modifier::DIM)
        );
        // An old date is open again.
        assert!(lines[4].contains("• Menu"), "{lines:#?}");
        assert!(!buffer[(7, 4)].modifier.contains(Modifier::CROSSED_OUT));
        assert!(lines[6].contains("home"), "{lines:#?}");

        // The body shows in the preview, like for tasks.
        app.cursor = 2;
        assert!(screen(&app).contains("https://menu"));
    }

    #[test]
    fn hint_without_sections() {
        let mut app = App::new(PathBuf::new(), sections(&[]));
        assert!(screen(&app).contains("no sections yet, N creates one"));

        // Below the daily checklist, if there is one.
        app.daily = Some(crate::daily::parse("## Jira\n"));
        let lines: Vec<String> = screen(&app).lines().map(str::to_string).collect();
        assert!(lines[3].contains("• Jira"), "{lines:#?}");
        assert!(lines[5].contains("no sections yet"), "{lines:#?}");

        app.sections = sections(&[("work", &[])]);
        assert!(!screen(&app).contains("no sections yet"));
    }

    #[test]
    fn list_highlights_filter_matches() {
        let mut app = App::new(
            PathBuf::new(),
            sections(&[("home", &["Fix typo", "Buy milk"]), ("work", &["Review"])]),
        );
        app.sections[1].tasks[0].body = "fix it".to_string();
        app.update(Action::Filter);
        for c in "fix".chars() {
            app.update(Action::FilterType(c));
        }
        let mut terminal = Terminal::new(TestBackend::new(50, 9)).unwrap();
        terminal
            .draw(|frame| {
                render(&app, frame);
            })
            .unwrap();
        let buffer = terminal.backend().buffer();
        let lines: Vec<String> = screen(&app).lines().map(str::to_string).collect();

        assert!(
            lines[2].trim_end().ends_with(" 1"),
            "only shown tasks count"
        );
        // `• Fix typo` starts at column 5, the match in bold yellow, the rest plain.
        assert!(lines[3].contains("• Fix typo"), "{lines:#?}");
        assert_eq!(buffer[(7, 3)].style().fg, Some(Color::Yellow));
        assert!(buffer[(7, 3)].modifier.contains(Modifier::BOLD));
        assert_eq!(buffer[(11, 3)].style().fg, Some(Color::Reset));
        // Matched in its body only: dim, with a hint.
        assert!(lines[6].contains("• Review …"), "{lines:#?}");
        assert!(buffer[(7, 6)].modifier.contains(Modifier::DIM));
    }

    #[test]
    fn preview_highlights_filter_matches() {
        let text = body_text("# Notes\nfix the FIX\n", "fix");
        let line = &text.lines[1];
        let highlighted: Vec<&str> = line
            .spans
            .iter()
            .filter(|span| span.style.fg == Some(Color::Yellow))
            .map(|span| span.content.as_ref())
            .collect();
        assert_eq!(highlighted, ["fix", "FIX"]);
        assert_eq!(line.to_string(), "fix the FIX");
    }

    #[test]
    fn preview_hidden_for_empty_task_and_section() {
        let mut app = App::new(PathBuf::new(), sections(&[("home", &["Fix typo"])]));
        app.sections[0].tasks[0].body = "\n  \n".to_string();

        // On the section header, then on the task with a blank body.
        for _ in 0..2 {
            let screen = screen(&app);
            assert!(!screen.contains('│'), "preview shown in\n{screen}");
            app.update(Action::Down);
        }
    }

    #[test]
    fn heatmap_below_the_list_once_something_is_archived() {
        let today = jiff::Zoned::now().date();
        let mut app = App::new(PathBuf::new(), sections(&[("home", &["Fix typo"])]));
        let lines = |app: &App, height| {
            let mut terminal = Terminal::new(TestBackend::new(60, height)).unwrap();
            terminal
                .draw(|frame| {
                    render(app, frame);
                })
                .unwrap();
            let buffer = terminal.backend().buffer();
            buffer
                .content()
                .chunks(buffer.area.width as usize)
                .map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>())
                .collect::<Vec<_>>()
        };
        let shown = |lines: &[String]| lines.iter().any(|line| line.trim_end().ends_with("Mon"));
        assert!(!shown(&lines(&app, 30)), "nothing archived yet");

        // Today the busiest, older days so it is not in the first column.
        app.days = [(today, 3), (today - 10.days(), 1), (today - 20.days(), 2)].into();
        let screen = lines(&app, 30);
        assert!(screen[2].contains("home"), "{screen:#?}");
        // A blank line, the labels and 7 rows above the status line.
        assert_eq!(screen[20].trim(), "", "{screen:#?}");
        assert!(screen[21].starts_with(" daily"), "{screen:#?}");
        let row = 22 + today.weekday().to_monday_zero_offset() as usize;
        assert!(
            screen[row].contains("[█]"),
            "today in brackets\n{screen:#?}"
        );
        assert!(screen[22].trim_end().ends_with("Mon"), "{screen:#?}");

        // The tasks come first: too short for both, the heatmap goes.
        assert!(!shown(&lines(&app, 20)));
    }
}
