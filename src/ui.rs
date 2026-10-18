//! Rendering: header, list, preview and status line. The help is in [`help`], the archive in
//! [`archive`], the experimental daily checklist in [`daily`]. Reads the app, never changes it.

use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style, Stylize},
    text::{Line, Span, Text},
    widgets::{Block, Borders, List, ListItem, ListState, Padding, Paragraph, Wrap},
};

mod archive;
mod daily;
mod help;

use crate::{
    app::{App, Page, Row, Status},
    filter,
    keymap::{self, Action, BINDINGS, FILTER_INPUT_HINTS},
};

/// What a frame leaves for the next one, the caller keeps it in the app.
pub struct Drawn {
    /// See [`App::list_offset`].
    pub list_offset: usize,
    /// See [`crate::app::archive::ArchivePage::columns`].
    pub heatmap_columns: usize,
    /// See [`crate::app::archive::ArchivePage::offset`].
    pub archive_offset: usize,
}

/// Draws the whole screen from the current state. Rendering never mutates the app, what the
/// next frame needs comes back in [`Drawn`].
pub fn render(app: &App, frame: &mut Frame) -> Drawn {
    // The header gets a blank line below, like the one above every section. A rule keeps the
    // status line apart, so its messages and hints are not taken for the last list row.
    let [header_area, gap_area, main_area, rule_area, status_area] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(0),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .areas(frame.area());
    // Before the preview, so its divider can join it.
    let rule = "─".repeat(rule_area.width as usize).fg(LINE_COLOR);
    frame.render_widget(Line::from(rule), rule_area);
    let mut drawn = Drawn {
        list_offset: app.list_offset,
        heatmap_columns: app.archive.columns,
        archive_offset: app.archive.offset,
    };
    match app.page {
        Page::Todo => {
            // Before the preview, so its divider can join it.
            let today = jiff::Zoned::now().strftime("%a %d %b").to_string();
            render_header(app, frame, header_area, &today);
            drawn.list_offset =
                render_todo(app, frame, [header_area, gap_area, main_area, rule_area]);
        }
        Page::Archive => {
            (drawn.heatmap_columns, drawn.archive_offset) = archive::render(app, frame, main_area);
            let selected = archive::range(app, drawn.heatmap_columns);
            render_header(app, frame, header_area, &selected);
        }
    }
    render_status(app, frame, status_area);
    if app.show_help {
        help::render(frame, app.page);
    }
    drawn
}

/// The list, and the preview of a task with a body next to it. Returns the list's offset.
fn render_todo(
    app: &App,
    frame: &mut Frame,
    [header_area, gap_area, main_area, rule_area]: [Rect; 4],
) -> usize {
    let [list_area, preview_area] =
        Layout::horizontal([Constraint::Percentage(40), Constraint::Percentage(60)])
            .areas(main_area);
    // Rules keep the split width even when the list gets the whole screen,
    // so they don't jump while moving between tasks with and without a body.
    let rule_width = list_block().inner(list_area).width as usize;

    // The preview only shows up for a task with a body, otherwise the list takes all the width.
    let body = match app.selected() {
        Some(Row::Task(s, t)) => Some(app.sections[s].tasks[t].body.as_str()),
        Some(Row::DailyTask(i)) => Some(daily::body(app, i)),
        _ => None,
    };
    match body {
        Some(body) if !body.trim().is_empty() => {
            render_preview(app, frame, preview_area, body);
            connect_divider(
                frame,
                preview_area.x,
                header_area.y,
                gap_area.y,
                rule_area.y,
            );
            render_list(app, frame, list_area, rule_width)
        }
        _ => render_list(app, frame, main_area, rule_width),
    }
}

/// Joins the preview divider at column `x` to the header: `┬` in the header rule, `│` in the
/// blank line below. Only where the rule is a line, never over the date or the counts. At the
/// bottom `┴` in the rule above the status line.
fn connect_divider(frame: &mut Frame, x: u16, header_y: u16, gap_y: u16, bottom_y: u16) {
    let buffer = frame.buffer_mut();
    buffer[(x, bottom_y)].set_symbol("┴");
    if buffer[(x, header_y)].symbol() == "─" {
        buffer[(x, header_y)].set_symbol("┬");
    }
    buffer[(x, gap_y)].set_symbol("│").set_fg(LINE_COLOR);
}

/// `── [ todo  archive ] ── Sat 17 Oct ──── 4 open · ✓ 2 done` over the whole width: the pages
/// with the current one bold, between the keys that switch them, the page's `date`, the tasks in
/// all sections and those archived today, so done tasks don't just vanish.
fn render_header(app: &App, frame: &mut Frame, area: Rect, date: &str) {
    let open: usize = app.sections.iter().map(|section| section.tasks.len()).sum();
    let done = app.done_today();

    let count = vec![
        format!("{open} open · ").dim(),
        "✓".fg(DONE_COLOR),
        format!(" {done} done").dim(),
    ];
    // The keys in the style of the hints in the status line.
    let mut pages = vec!["── ".fg(LINE_COLOR), "[ ".bold().fg(HEADER_COLOR)];
    for (i, page) in Page::ALL.into_iter().enumerate() {
        if i > 0 {
            pages.push("  ".into());
        }
        pages.push(match page == app.page {
            true => page.name().bold(),
            false => page.name().dim(),
        });
    }
    pages.extend([" ]".bold().fg(HEADER_COLOR), " ── ".fg(LINE_COLOR)]);
    let spans = rule(Some(pages), date, count, HEADER_COLOR, area.width as usize);
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

    // Between the hints, and at least that much between the two sides.
    const GAP: &str = "  ";
    let line = |hints: &[(String, &str)]| {
        let mut spans = Vec::new();
        for (i, (key, text)) in hints.iter().enumerate() {
            if i > 0 {
                spans.push(GAP.into());
            }
            spans.push(key.clone().bold().fg(HEADER_COLOR));
            spans.push(format!(" {text}").dim());
        }
        Line::from(spans)
    };
    // Hints give way from the end, the left side is what just happened.
    let mut hints = hints(app);
    while !hints.is_empty() && left.width() + GAP.len() + line(&hints).width() > area.width as usize
    {
        hints.pop();
    }

    frame.render_widget(left, area);
    frame.render_widget(line(&hints).right_aligned(), area);
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
    let hints = if app.filter.is_empty() || app.show_help || app.page == Page::Archive {
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
        (Some(Status::Error(message)), _) => Line::from(message.as_str().red().bold()),
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

/// The date in the header.
const HEADER_COLOR: Color = Color::Blue;

/// The check for tasks done today.
const DONE_COLOR: Color = Color::Green;

/// Rules and the preview divider.
const LINE_COLOR: Color = Color::DarkGray;

fn section_color(index: usize) -> Color {
    SECTION_COLORS[index % SECTION_COLORS.len()]
}

/// Opens the section key in a section rule: `──[1]─ work`.
const KEY_OPEN: &str = "──[";

/// `──[1]─ work ─────── 2` for section `s` in its color, filled to `width`. The todo list and
/// the done tasks on the archive page.
fn section_rule(s: usize, name: &str, count: usize, width: usize) -> Vec<Span<'static>> {
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
    let lead = match keymap::jump_key(s) {
        Some(key) => keyed(key),
        None => {
            let lead_width: usize = keyed(' ').iter().map(Span::width).sum();
            vec![format!("{} ", "─".repeat(lead_width - 1)).fg(LINE_COLOR)]
        }
    };
    let count = vec![count.to_string().dim()];
    rule(Some(lead), name, count, section_color(s), width)
}

/// Gutter and indent before a task title, the bullet below the key of the section. `first` is
/// the first line of the title, the bullet only goes there.
fn task_prefix(gutter: Span<'static>, color: Color, first: bool) -> Vec<Span<'static>> {
    let bullet = if first { "• ".fg(color) } else { "  ".into() };
    vec![gutter, " ".repeat(KEY_OPEN.chars().count()).into(), bullet]
}

/// No box around the list. Sections are rules, tasks get a bullet in the section color,
/// the cursor is a bar in the gutter. Returns the scroll offset.
fn render_list(app: &App, frame: &mut Frame, area: Rect, width: usize) -> usize {
    let block = list_block();
    let text_width = block.inner(area).width as usize;

    let mut items: Vec<ListItem> = Vec::new();
    let mut selected = 0;
    let rows = app.rows();
    for (i, &row) in rows.iter().enumerate() {
        let is_cursor = i == app.cursor;
        let color = match row {
            Row::Section(s) | Row::Task(s, _) => section_color(s),
            Row::Daily | Row::DailyTask(_) => daily::COLOR,
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
                // Only the shown tasks, so with a filter it says how many match.
                let count = rows
                    .iter()
                    .filter(|row| matches!(row, Row::Task(task_s, _) if *task_s == s))
                    .count();
                let mut spans = vec![gutter()];
                spans.extend(section_rule(
                    s,
                    &app.sections[s].name,
                    count,
                    width.saturating_sub(gutter().width()),
                ));
                items.push(ListItem::new(Line::from(spans)));
                continue;
            }
            Row::Daily => {
                let mut spans = vec![gutter()];
                spans.extend(daily::header(app, width.saturating_sub(gutter().width())));
                items.push(ListItem::new(Line::from(spans)));
                continue;
            }
            Row::DailyTask(i) => daily::task(app, i, task_style),
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

        let prefix = |first: bool| task_prefix(gutter(), color, first);
        let title_width = text_width.saturating_sub(Line::from(prefix(true)).width());
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
/// `lead` replaces the `── ` before the title, `count` goes after the fill as styled.
fn rule(
    lead: Option<Vec<Span<'static>>>,
    title: &str,
    mut count: Vec<Span<'static>>,
    color: Color,
    width: usize,
) -> Vec<Span<'static>> {
    let mut spans = lead.unwrap_or_else(|| vec!["── ".fg(LINE_COLOR)]);
    spans.push(title.to_string().fg(color).bold());
    spans.push(" ".into());
    if !count.is_empty() {
        count.insert(0, " ".into());
    }
    let used: usize = spans.iter().chain(&count).map(Span::width).sum();
    spans.push("─".repeat(width.saturating_sub(used)).fg(LINE_COLOR));
    spans.extend(count);
    spans
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
    use ratatui::{Terminal, backend::TestBackend, style::Modifier};

    /// Renders the app and returns the screen as text, styles (bold, highlight) are ignored.
    pub(super) fn screen(app: &App) -> String {
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
        terminal
            .draw(|frame| offset = render(app, frame).list_offset)
            .unwrap();
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
        assert!(lines[6].contains("▌   • Task 20"), "{lines:#?}");

        // Moving up inside the screen moves the cursor, not the view.
        app.update(Action::Up);
        draw(&mut app);
        app.update(Action::Up);
        let lines = draw(&mut app);
        assert!(lines[6].contains("• Task 20"), "{lines:#?}");
        assert!(lines[4].contains("▌   • Task 18"), "{lines:#?}");
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
    fn preview_divider_joins_the_header_and_the_rule() {
        let mut app = App::new(
            PathBuf::new(),
            sections(&[("home", &["Fix typo", "Empty"])]),
        );
        app.sections[0].tasks[0].body = "line one".to_string();
        let column = |line: &str, c: char| line.chars().position(|x| x == c);

        app.update(Action::Down);
        let lines: Vec<String> = screen(&app).lines().map(str::to_string).collect();
        let divider = column(&lines[2], '│').expect("preview divider");
        // 50 wide, the pages and the date in the header cover it, it is never drawn over them.
        assert_eq!(column(&lines[0], '┬'), None, "{lines:#?}");
        assert_eq!(column(&lines[1], '│'), Some(divider), "{lines:#?}");
        assert_eq!(column(&lines[7], '┴'), Some(divider), "{lines:#?}");

        // Without a preview the header and the rule are plain lines.
        app.update(Action::Down);
        let lines: Vec<String> = screen(&app).lines().map(str::to_string).collect();
        assert!(!lines[0].contains('┬'), "{lines:#?}");
        assert_eq!(lines[1].trim(), "");
        assert!(!lines[7].contains('┴'), "{lines:#?}");
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
        app.days = [(jiff::Zoned::now().date(), 2)].into();
        // Wider than the others, the pages take room.
        let mut terminal = Terminal::new(TestBackend::new(60, 9)).unwrap();
        terminal
            .draw(|frame| {
                render(&app, frame);
            })
            .unwrap();
        let buffer = terminal.backend().buffer();
        let lines: Vec<String> = buffer
            .content()
            .chunks(buffer.area.width as usize)
            .map(|row| row.iter().map(|cell| cell.symbol()).collect())
            .collect();

        let date = jiff::Zoned::now().strftime("%a %d %b").to_string();
        assert!(
            lines[0].starts_with(&format!("── [ todo  archive ] ── {date} ─")),
            "{lines:#?}"
        );
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
}
