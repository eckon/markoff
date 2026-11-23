//! Rendering: the layout, the app header, and what the pages share. The task list is in
//! [`tasks`], the archive in [`archive`], the help in [`help`], the footer in [`footer`], the
//! preview in [`preview`]. Reads the app, never changes it.

use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style, Stylize},
    text::{Line, Span},
};

mod archive;
mod footer;
mod help;
mod preview;
mod rows;
pub mod spinner;
mod tasks;

use crate::{
    app::{App, Page},
    filter,
};

/// What a frame leaves for the next one, the caller keeps it in the app.
pub struct Drawn {
    /// See [`crate::app::TasksPage::offset`].
    pub list_offset: usize,
    /// See [`crate::app::Help::offset`], `None` without the help.
    pub help_offset: Option<usize>,
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
    footer::render(app, frame, footer_area);
    let mut drawn = Drawn {
        list_offset: app.tasks_page.offset,
        help_offset: None,
        heatmap_offset: app.archive_page.heatmap_offset,
        archive_offset: app.archive_page.offset,
    };
    match app.page {
        Page::Tasks => {
            render_app_header(app, frame, app_header_area, Some(tasks::app_header(app)));
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
            render_app_header(app, frame, app_header_area, archive::app_header(app));
        }
    }
    if let Some(help) = &app.help {
        drawn.help_offset = Some(help::render(frame, app.page, &help.filter, help.offset));
    }
    drawn
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

/// `── tasks  archive ── Sat 17 Oct ──── 4 open · ✓ 2 done · ✗ 1 canceled` over the whole
/// width: the pages with the current one in [`CURRENT_COLOR`], then the page's date and counts.
/// Without them the line runs on right after the pages.
fn render_app_header(
    app: &App,
    frame: &mut Frame,
    area: Rect,
    date_and_count: Option<(String, Vec<Span<'static>>)>,
) {
    let (date, count) = date_and_count.unzip();
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
    pages.push(match date {
        Some(_) => " ── ".fg(LINE_COLOR),
        None => " ".into(),
    });
    // Not bold, that is for keys.
    let spans = filled_line(
        Some(pages),
        date.unwrap_or_default().into(),
        count.unwrap_or_default(),
        area.width as usize,
    );
    frame.render_widget(Line::from(spans), area);
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
    if !title.content.is_empty() {
        spans.push(title);
        spans.push(" ".into());
    }
    if !count.is_empty() {
        count.insert(0, " ".into());
    }
    let used: usize = spans.iter().chain(&count).map(Span::width).sum();
    spans.push("─".repeat(width.saturating_sub(used)).fg(LINE_COLOR));
    spans.extend(count);
    spans
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

    use crate::app::tests::{TODAY, sections};
    use ratatui::{Terminal, backend::TestBackend, buffer::Buffer};

    /// Renders the app `width` wide and `height` high, the buffer has the styles.
    pub(super) fn draw(app: &App, width: u16, height: u16) -> Buffer {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|frame| _ = render(app, frame)).unwrap();
        terminal.backend().buffer().clone()
    }

    /// The lines of text in `buffer`, styles (bold, highlight) are ignored.
    pub(super) fn screen_lines(buffer: &Buffer) -> Vec<String> {
        (buffer.content().chunks(buffer.area.width as usize))
            .map(|row| row.iter().map(|cell| cell.symbol()).collect())
            .collect()
    }

    /// Renders the app 50 by 9 and returns the screen as text.
    pub(super) fn screen(app: &App) -> String {
        screen_lines(&draw(app, 50, 9)).join("\n")
    }

    #[test]
    fn header_shows_date_open_and_done() {
        let mut app = App::new(
            PathBuf::new(),
            sections(&[("home", &["Fix typo"]), ("work", &["Review", "Deploy"])]),
            crate::config::defaults(),
        );
        app.today = TODAY;
        let done = |content: &str| crate::archive::tests::done("home", content);
        app.archive = [(TODAY, vec![done("# A"), done("# B"), done("# ~~C~~")])].into();
        app.tasks_page.hide_preview = true;
        // Wider than the others, the pages take room.
        let buffer = draw(&app, 70, 9);
        let lines = screen_lines(&buffer);

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
        assert_eq!(buffer[(check, 0)].fg, DONE_COLOR);
        let cross = lines[0].chars().position(|c| c == '✗').unwrap() as u16;
        assert_eq!(buffer[(cross, 0)].fg, CANCELED_COLOR);

        // A blank line, then the sections.
        assert_eq!(lines[1].trim(), "");
        assert!(lines[2].contains("home"), "{lines:#?}");

        // Each count only when there are any.
        app.archive = [(TODAY, vec![done("# ~~C~~")])].into();
        let first_line = |app: &App| screen_lines(&draw(app, 70, 9)).remove(0);
        assert!(first_line(&app).ends_with("─ 3 open · ✗ 1 canceled"));
        app.archive.clear();
        assert!(first_line(&app).ends_with("─ 3 open"));
    }
}
