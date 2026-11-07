//! Rendering: app header, preview and footer. The task list is in [`tasks`], the help in
//! [`help`], the archive in [`archive`]. Reads the app, never changes it.

use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style, Stylize},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Padding, Paragraph, Wrap},
};

mod archive;
mod help;
mod rows;
mod tasks;

use crate::{
    app::{
        App, Page, Row, Status,
        archive::{ArchiveRow, Focus},
    },
    filter,
    keymap::{Action, BINDINGS, Context},
};

/// What a frame leaves for the next one, the caller keeps it in the app.
pub struct Drawn {
    /// See [`App::list_offset`].
    pub list_offset: usize,
    /// See [`App::help_offset`].
    pub help_offset: usize,
    /// See [`crate::app::archive::ArchivePage::heatmap_offset`].
    pub heatmap_offset: usize,
    /// See [`crate::app::archive::ArchivePage::offset`].
    pub archive_offset: usize,
}

/// Draws the whole screen from the current state. Rendering never mutates the app, what the
/// next frame needs comes back in [`Drawn`].
pub fn render(app: &App, frame: &mut Frame) -> Drawn {
    // The app header gets a blank line below, like the one above every section, the footer one
    // above, so the two lines frame the page alike.
    let [app_header_area, top_gap, main_area, bottom_gap, footer_area] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(0),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .areas(frame.area());
    // Before the preview, so its divider can join it.
    render_footer(app, frame, footer_area);
    let mut drawn = Drawn {
        list_offset: app.list_offset,
        help_offset: app.help_offset,
        heatmap_offset: app.archive_page.heatmap_offset,
        archive_offset: app.archive_page.offset,
    };
    match app.page {
        Page::Tasks => {
            // The tasks in all sections and those archived today, so done tasks don't just
            // vanish.
            let today = app.today.strftime("%a %d %b").to_string();
            // Hidden tasks are not open, even while `.` shows them.
            let open = (app.sections.iter().flat_map(|section| &section.tasks))
                .filter(|task| task.due(app.today))
                .count();
            let mut count = vec![format!("{open} open").dim()];
            let done = done_count(app.done_today());
            if !done.is_empty() {
                count.push(" · ".dim());
                count.extend(done);
            }
            render_app_header(app, frame, app_header_area, &today, count);
            let preview;
            (drawn.list_offset, preview) = tasks::render(app, frame, main_area);
            if let Some(x) = preview {
                join_divider(frame, x, top_gap.y, app_header_area.y, "┬");
                join_divider(frame, x, bottom_gap.y, footer_area.y, "┴");
            }
        }
        Page::Archive => {
            let divider;
            (drawn.heatmap_offset, drawn.archive_offset, divider) =
                archive::render(app, frame, main_area);
            if let Some(x) = divider {
                join_divider(frame, x, bottom_gap.y, footer_area.y, "┴");
            }
            // The tasks done on the selected days.
            let range = archive::range(app);
            let done: Vec<&crate::archive::Done> = (app.archive_rows().into_iter())
                .filter_map(|row| match row {
                    ArchiveRow::Task(_, day, index) => Some(&app.archive[&day][index]),
                    ArchiveRow::Section(..) => None,
                })
                .collect();
            render_app_header(app, frame, app_header_area, &range, done_count(done));
        }
    }
    if app.show_help {
        drawn.help_offset = help::render(frame, app.page, &app.help_filter, app.help_offset);
    }
    drawn
}

/// A list in `area` drawn by `list`, and next to it the preview of `body` with the matches of
/// `filter`, also when it is empty: the list keeps its width while moving, so wrapped titles
/// stay put. Without a preview (`None`) the list takes all the width, its section headers keep
/// the split width. Returns the list's offset and where the preview went.
fn list_with_preview(
    frame: &mut Frame,
    area: Rect,
    body: Option<&str>,
    filter: &str,
    list: impl FnOnce(&mut Frame, Rect, usize) -> usize,
) -> (usize, Option<Rect>) {
    let [list_area, preview_area] =
        Layout::horizontal([Constraint::Percentage(40), Constraint::Percentage(60)]).areas(area);
    let header_width = rows::block().inner(list_area).width as usize;
    match body {
        Some(body) => {
            render_preview(frame, preview_area, body, filter);
            (list(frame, list_area, header_width), Some(preview_area))
        }
        None => (list(frame, area, header_width), None),
    }
}

/// `✓ 2 done · ✗ 1 canceled` for the app header, each only if there are any.
fn done_count<'a>(tasks: impl IntoIterator<Item = &'a crate::archive::Done>) -> Vec<Span<'static>> {
    let (canceled, done): (Vec<_>, Vec<_>) = tasks.into_iter().partition(|done| done.task.canceled);
    let mut spans = Vec::new();
    if !done.is_empty() {
        spans.extend(["✓".fg(DONE_COLOR), format!(" {} done", done.len()).dim()]);
    }
    if !canceled.is_empty() {
        if !spans.is_empty() {
            spans.push(" · ".dim());
        }
        spans.extend([
            "✗".fg(CANCELED_COLOR),
            format!(" {} canceled", canceled.len()).dim(),
        ]);
    }
    spans
}

/// Joins the preview divider at column `x` to the line in row `line_y`: `│` in the blank line
/// between, `joint` in the line. Only where it is drawn, never over the date, the counts,
/// the status or the hints.
fn join_divider(frame: &mut Frame, x: u16, gap_y: u16, line_y: u16, joint: &str) {
    let buffer = frame.buffer_mut();
    buffer[(x, gap_y)].set_symbol("│").set_fg(LINE_COLOR);
    if buffer[(x, line_y)].symbol() == "─" {
        buffer[(x, line_y)].set_symbol(joint);
    }
}

/// `── tasks  archive ── Sat 17 Oct ──── 4 open · ✓ 2 done · ✗ 1 canceled` over the whole width: the pages with
/// the current one in [`CURRENT_COLOR`], then the page's `date` and `count`.
fn render_app_header(
    app: &App,
    frame: &mut Frame,
    area: Rect,
    date: &str,
    count: Vec<Span<'static>>,
) {
    let mut pages = vec!["── ".fg(LINE_COLOR)];
    for (i, page) in Page::ALL.into_iter().enumerate() {
        if i > 0 {
            pages.push("  ".into());
        }
        pages.push(match page == app.page {
            true => page.name().fg(CURRENT_COLOR).bold(),
            false => page.name().dim(),
        });
    }
    pages.push(" ── ".fg(LINE_COLOR));
    // Not bold, that is for keys.
    let spans = filled_line(
        Some(pages),
        date.to_string().into(),
        count,
        area.width as usize,
    );
    frame.render_widget(Line::from(spans), area);
}

/// `── what just happened ──── hints` over the whole width, a line like the app header: left the
/// filter input or the status, right the key hints. Without either side the line runs
/// to the edge.
fn render_footer(app: &App, frame: &mut Frame, area: Rect) {
    let left = match app.filter_input {
        true => vec![format!("/{}", app.active_filter()).into()],
        false => status_text(app),
    };
    let mut spans = Vec::new();
    if !left.is_empty() {
        spans.push("── ".fg(LINE_COLOR));
        spans.extend(left);
        spans.push(" ".into());
    }
    let used: usize = spans.iter().map(Span::width).sum();
    if app.filter_input {
        let end = area.x + used.saturating_sub(1) as u16;
        frame.set_cursor_position((end.min(area.right().saturating_sub(1)), area.y));
    }

    // Between the hints, and at least that much line between the two sides.
    const GAP: usize = 2;
    let hint_spans = |hints: &[(String, &str)]| {
        let mut spans: Vec<Span> = Vec::new();
        for (i, (key, text)) in hints.iter().enumerate() {
            spans.push(if i == 0 { " " } else { "  " }.into());
            spans.push(key.clone().bold());
            spans.push(format!(" {text}").dim());
        }
        spans
    };
    let width = |spans: &[Span]| spans.iter().map(Span::width).sum::<usize>();
    // Hints give way from the end, the left side is what just happened.
    let mut hints = hints(app);
    while !hints.is_empty() && used + GAP + width(&hint_spans(&hints)) > area.width as usize {
        hints.pop();
    }
    let hints = hint_spans(&hints);

    let fill = (area.width as usize).saturating_sub(used + width(&hints));
    spans.push("─".repeat(fill).fg(LINE_COLOR));
    spans.extend(hints);
    frame.render_widget(Line::from(spans), area);
}

/// Key hints per mode: how to get out of the filter, and where to find the rest. Keys from
/// [`BINDINGS`], so they always match. The filter input's with their texts, the others with own
/// ones that fit the footer.
fn hints(app: &App) -> Vec<(String, &'static str)> {
    let binding = |context, action| {
        BINDINGS
            .iter()
            .find(|b| b.contexts.contains(&context) && b.keys.iter().any(|(_, a)| *a == action))
    };
    if app.filter_input {
        return [Action::FilterAccept, Action::FilterClear]
            .into_iter()
            .filter_map(|action| binding(Context::Filter, action))
            .map(|b| (b.labels().join(" / "), b.help))
            .collect();
    }
    // The page's keys, also while the help is open.
    let key = |action| binding(Context::Page(app.page), action).map(|b| b.labels().join(" / "));
    let mut hints = Vec::new();
    // The filter is cleared from its input: its key, then `Esc`.
    if !app.active_filter().is_empty() {
        hints.extend(key(Action::Filter).map(|key| (format!("{key} Esc"), "clear filter")));
    }
    // A range is ended with the key that started it or `Esc`, in the heatmap.
    if app.page == Page::Archive
        && app.archive_page.anchor.is_some()
        && app.archive_page.focus == Focus::Heatmap
        && !app.show_help
    {
        hints.extend(key(Action::Visual).map(|key| (format!("{key} / Esc"), "end range")));
    }
    hints.extend(key(Action::Help).map(|key| (key, "toggle help")));
    hints
}

/// The status, left in the footer when the filter input is closed, empty for nothing.
fn status_text(app: &App) -> Vec<Span<'_>> {
    match (&app.status, app.keymap.pending()) {
        (Some(Status::Error(message)), _) => vec![message.as_str().red().bold()],
        (Some(Status::Info(message)), _) => vec![message.as_str().into()],
        (None, Some(key)) => vec![key.to_string().into()],
        (None, None) if app.show_help && !app.help_filter.is_empty() => {
            let count = help::bindings(app.page, &app.help_filter).len();
            let keys = if count == 1 { "key" } else { "keys" };
            vec![
                "filter: ".dim(),
                app.help_filter.as_str().into(),
                format!(" · {count} {keys}").dim(),
            ]
        }
        (None, None) if !app.page_filter().is_empty() => {
            // The archive counts its matches over all days, not only the selected ones.
            let count = match app.archive_matches() {
                Some(matches) if app.page == Page::Archive => matches.values().sum(),
                _ => app
                    .rows()
                    .iter()
                    .filter(|row| matches!(row, Row::Task(..)))
                    .count(),
            };
            let tasks = if count == 1 { "task" } else { "tasks" };
            vec![
                "filter: ".dim(),
                app.page_filter().into(),
                format!(" · {count} {tasks}").dim(),
            ]
        }
        (None, None) => Vec::new(),
    }
}

/// Colors of the sections in list order, repeating. Only the 16 terminal
/// colors, so they follow the terminal theme.
const SECTION_COLORS: [Color; 4] = [Color::Blue, Color::Magenta, Color::Green, Color::Yellow];

/// Where you are: the current page and the selected heatmap cell.
const CURRENT_COLOR: Color = Color::Blue;

/// The check for tasks done today.
const DONE_COLOR: Color = Color::Green;

/// The cross for tasks canceled today.
const CANCELED_COLOR: Color = Color::Red;

/// Rules and the preview divider.
const LINE_COLOR: Color = Color::DarkGray;

fn section_color(index: usize) -> Color {
    SECTION_COLORS[index % SECTION_COLORS.len()]
}

/// Styled chars joined back into spans, one per run of the same style.
fn spans(chars: &[(char, Style)]) -> Vec<Span<'static>> {
    chars
        .chunk_by(|a, b| a.1 == b.1)
        .map(|run| Span::styled(run.iter().map(|(c, _)| c).collect::<String>(), run[0].1))
        .collect()
}

/// `── work ─────── 2`, filled to `width`. Section headers, the app header, group headers.
/// `lead` replaces the `── ` before the title, `count` goes after the fill as styled.
fn filled_line(
    lead: Option<Vec<Span<'static>>>,
    title: Span<'static>,
    mut count: Vec<Span<'static>>,
    width: usize,
) -> Vec<Span<'static>> {
    let mut spans = lead.unwrap_or_else(|| vec!["── ".fg(LINE_COLOR)]);
    spans.push(title);
    spans.push(" ".into());
    if !count.is_empty() {
        count.insert(0, " ".into());
    }
    let used: usize = spans.iter().chain(&count).map(Span::width).sum();
    spans.push("─".repeat(width.saturating_sub(used)).fg(LINE_COLOR));
    spans.extend(count);
    spans
}

/// Task body as raw text, read only, with the matches of `filter`. No title, the list shows it
/// in full. The task list and the archive page.
fn render_preview(frame: &mut Frame, area: Rect, body: &str, filter: &str) {
    let text = match body.trim().is_empty() {
        true => Text::from("<empty>".dim()),
        false => body_text(body, filter),
    };

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

/// A task's title in a list, in `style` with the matches of `filter`. Listed for a match in the
/// body only: weaker, `…` hints at the preview.
fn filtered_title(title: &str, filter: &str, style: Style) -> Vec<Span<'static>> {
    let body_only = !filter::matches(title, filter);
    let style = if body_only { style.dim() } else { style };
    let mut spans = highlight(title, filter, style);
    if body_only {
        spans.push(" …".dim());
    }
    spans
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

    use crate::{
        app::tests::{TODAY, sections},
        keymap::Action,
    };
    use ratatui::{Terminal, backend::TestBackend};

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

    #[test]
    fn preview_shows_task_body() {
        let mut app = App::new(
            PathBuf::new(),
            sections(&[("home", &["Fix typo"])]),
            crate::config::defaults(),
        );
        app.sections[0].tasks[0].body = "line one\n## Notes\nline two".to_string();
        app.update(Action::Down);

        let screen = screen(&app);
        for text in ["line one", "## Notes", "line two"] {
            assert!(screen.contains(text), "missing {text:?} in\n{screen}");
        }
        // The title is only in the list, the preview starts with the body.
        assert_eq!(screen.matches("Fix typo").count(), 1, "{screen}");

        // `p` hides it, the title gets all the width.
        app.update(Action::Preview);
        let hidden = super::tests::screen(&app);
        assert!(!hidden.contains("line one"), "{hidden}");
        assert!(!hidden.contains('│'), "{hidden}");
    }

    #[test]
    fn preview_divider_joins_the_app_header_and_the_footer() {
        let mut app = App::new(
            PathBuf::new(),
            sections(&[("home", &["Fix typo", "Empty"])]),
            crate::config::defaults(),
        );
        app.sections[0].tasks[0].body = "line one".to_string();
        let column = |line: &str, c: char| line.chars().position(|x| x == c);

        app.update(Action::Down);
        let lines: Vec<String> = screen(&app).lines().map(str::to_string).collect();
        let divider = column(&lines[2], '│').expect("preview divider");
        // 50 wide, the pages and the date in the app header cover it, it is never drawn over them.
        assert_eq!(column(&lines[0], '┬'), None, "{lines:#?}");
        assert_eq!(column(&lines[1], '│'), Some(divider), "{lines:#?}");
        assert_eq!(column(&lines[7], '│'), Some(divider), "{lines:#?}");
        assert_eq!(column(&lines[8], '┴'), Some(divider), "{lines:#?}");

        // On a task without a body the preview stays, the list keeps its width.
        app.update(Action::Down);
        let lines: Vec<String> = screen(&app).lines().map(str::to_string).collect();
        assert_eq!(column(&lines[2], '│'), Some(divider), "{lines:#?}");

        // Without a preview the app header and the footer are plain lines.
        app.update(Action::Preview);
        let lines: Vec<String> = screen(&app).lines().map(str::to_string).collect();
        assert!(!lines[0].contains('┬'), "{lines:#?}");
        assert_eq!(lines[1].trim(), "");
        assert_eq!(lines[7].trim(), "");
        assert!(!lines[8].contains('┴'), "{lines:#?}");
    }

    #[test]
    fn status_line_shows_errors() {
        let mut app = App::new(
            PathBuf::new(),
            sections(&[("home", &[])]),
            crate::config::defaults(),
        );
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
            crate::config::defaults(),
        );
        app.update(Action::Filter);
        for c in "rev".chars() {
            app.update(Action::FilterType(c));
        }
        let lines: Vec<String> = screen(&app).lines().map(str::to_string).collect();
        // The first visible section starts right below the app header, without another blank line.
        assert!(lines[2].contains("work"), "{lines:#?}");
        assert!(lines[3].contains("Review"), "{lines:#?}");
        assert!(!lines.join("\n").contains("Fix typo"));
        assert!(lines[8].starts_with("── /rev ─"), "input at the bottom");
        assert!(lines[8].ends_with("<CR> keep filter  Esc clear filter"));

        // Accepted, the filter stays visible in the footer, with its matches.
        app.update(Action::FilterAccept);
        let screen = screen(&app);
        let status = screen.lines().last().unwrap();
        assert!(
            status.starts_with("── filter: rev · 1 task ─"),
            "{status:?}"
        );
        // `? toggle help` gives way, 50 columns are too narrow for both hints.
        assert!(status.ends_with("Esc clear filter"), "{status:?}");
    }

    #[test]
    fn status_hints_give_way_to_messages() {
        let mut app = App::new(
            PathBuf::new(),
            sections(&[("home", &[])]),
            crate::config::defaults(),
        );
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
        assert_eq!(
            screen(&app).lines().last().unwrap(),
            format!("── {message} ──")
        );
    }

    #[test]
    fn header_shows_date_open_and_done() {
        let mut app = App::new(
            PathBuf::new(),
            sections(&[("home", &["Fix typo"]), ("work", &["Review", "Deploy"])]),
            crate::config::defaults(),
        );
        app.today = TODAY;
        let done = |content: &str| crate::archive::Done {
            section: "home".to_string(),
            task: crate::task::Task::parse(PathBuf::new(), content),
        };
        app.archive = [(TODAY, vec![done("# A"), done("# B"), done("# ~~C~~")])].into();
        app.hide_preview = true;
        // Wider than the others, the pages take room.
        let mut terminal = Terminal::new(TestBackend::new(70, 9)).unwrap();
        let draw = |terminal: &mut Terminal<TestBackend>, app: &App| {
            terminal.draw(|frame| _ = render(app, frame)).unwrap();
            let buffer = terminal.backend().buffer();
            (buffer.content().chunks(buffer.area.width as usize))
                .map(|row| row.iter().map(|cell| cell.symbol()).collect())
                .collect::<Vec<String>>()
        };
        let lines = draw(&mut terminal, &app);

        assert!(
            lines[0].starts_with(&format!(
                "── tasks  archive ── {} ─",
                TODAY.strftime("%a %d %b")
            )),
            "{lines:#?}"
        );
        assert!(
            lines[0].ends_with("─ 3 open · ✓ 2 done · ✗ 1 canceled"),
            "{lines:#?}"
        );
        let check = lines[0].chars().position(|c| c == '✓').unwrap() as u16;
        assert_eq!(terminal.backend().buffer()[(check, 0)].fg, DONE_COLOR);
        let cross = lines[0].chars().position(|c| c == '✗').unwrap() as u16;
        assert_eq!(terminal.backend().buffer()[(cross, 0)].fg, CANCELED_COLOR);

        // A blank line, then the sections.
        assert_eq!(lines[1].trim(), "");
        assert!(lines[2].contains("home"), "{lines:#?}");

        // Each count only when there are any.
        app.archive = [(TODAY, vec![done("# ~~C~~")])].into();
        assert!(draw(&mut terminal, &app)[0].ends_with("─ 3 open · ✗ 1 canceled"));
        app.archive.clear();
        assert!(draw(&mut terminal, &app)[0].ends_with("─ 3 open"));
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
    fn preview_without_a_body_says_so() {
        let mut app = App::new(
            PathBuf::new(),
            sections(&[("home", &["Fix typo"])]),
            crate::config::defaults(),
        );
        app.sections[0].tasks[0].body = "\n  \n".to_string();

        // On the section header, then on the task with a blank body.
        for _ in 0..2 {
            let screen = screen(&app);
            assert!(screen.contains("│ <empty>"), "{screen}");
            app.update(Action::Down);
        }

        // Without rows the note gets all the width.
        let app = App::new(PathBuf::new(), sections(&[]), crate::config::defaults());
        assert!(!screen(&app).contains('│'));
    }
}
