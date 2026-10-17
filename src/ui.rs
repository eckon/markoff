//! Rendering: list, preview, status line and popups. Reads the app, never changes it.

use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Color, Modifier, Style, Stylize},
    text::{Line, Span, Text},
    widgets::{
        Block, BorderType, Borders, Clear, List, ListItem, ListState, Padding, Paragraph, Wrap,
    },
};

use crate::{
    app::{App, Row, Status},
    filter,
    keymap::{BINDINGS, Binding},
};

/// Draws the whole screen from the current state. Rendering never mutates the app.
pub fn render(app: &App, frame: &mut Frame) {
    let [main_area, status_area] =
        Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).areas(frame.area());
    let [list_area, preview_area] =
        Layout::horizontal([Constraint::Percentage(40), Constraint::Percentage(60)])
            .areas(main_area);
    // Rules keep the split width even when the list gets the whole screen,
    // so they don't jump while moving between tasks with and without a body.
    let rule_width = list_block().inner(list_area).width as usize;

    // The preview only shows up for a task with a body, otherwise the list takes all the width.
    match app.selected() {
        Some(Row::Task(s, t)) if !app.sections[s].tasks[t].body.trim().is_empty() => {
            render_list(app, frame, list_area, rule_width);
            render_preview(app, frame, preview_area, s, t);
        }
        _ => render_list(app, frame, main_area, rule_width),
    }
    render_status(app, frame, status_area);
    if app.show_help {
        render_help(frame);
    }
    if !app.renamed.is_empty() {
        render_renamed(app, frame);
    }
}

/// Centered popup listing the renames done at startup, old name above the new one.
fn render_renamed(app: &App, frame: &mut Frame) {
    let mut lines = vec![
        Line::from("Files were renamed to match"),
        Line::from("their titles and order:"),
        Line::default(),
    ];

    // Borders, the 3 lines above and the 2 below take 7 rows, each rename takes 2.
    let room = (frame.area().height as usize).saturating_sub(7);
    let total = app.renamed.len();
    let shown = if total * 2 <= room {
        total
    } else {
        room.saturating_sub(1) / 2
    };
    for (from, to) in &app.renamed[..shown] {
        lines.push(Line::from(from.display().to_string()));
        lines.push(Line::from(format!("  -> {}", to.display())));
    }
    if shown < total {
        lines.push(Line::from(format!("… and {} more", total - shown)).dim());
    }
    lines.push(Line::default());
    lines.push(Line::from("any key closes this").dim().centered());

    let width = lines.iter().map(Line::width).max().unwrap_or(0) as u16 + 4;
    let height = lines.len() as u16 + 2;
    let area = frame
        .area()
        .centered(Constraint::Length(width), Constraint::Length(height));
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(lines).block(block(" renamed ".to_string()).padding(Padding::horizontal(1))),
        area,
    );
}

/// Centered popup listing [`BINDINGS`], so it always matches the real keys.
fn render_help(frame: &mut Frame) {
    let labels: Vec<String> = BINDINGS.iter().map(Binding::label).collect();
    let label_width = labels.iter().map(|l| l.chars().count()).max().unwrap_or(0);

    let mut lines: Vec<Line> = BINDINGS
        .iter()
        .zip(&labels)
        .map(|(binding, label)| {
            Line::from(vec![
                format!("{label:>label_width$}  ").bold(),
                binding.help.into(),
            ])
        })
        .collect();
    lines.push(Line::default());
    lines.push(Line::from("any key closes this help").dim().centered());

    let width = lines.iter().map(Line::width).max().unwrap_or(0) as u16 + 4;
    let height = lines.len() as u16 + 2;
    let area = frame
        .area()
        .centered(Constraint::Length(width), Constraint::Length(height));
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(lines).block(block(" help ".to_string()).padding(Padding::horizontal(1))),
        area,
    );
}

/// The open filter input with the terminal cursor at its end. Otherwise the last error or
/// message, the pending key of an unfinished sequence (`g`), or the filter that is set with
/// its number of matching tasks.
fn render_status(app: &App, frame: &mut Frame, area: Rect) {
    let filter = format!("/{}", app.filter);
    if app.filter_input {
        let end = area.x + Line::from(filter.as_str()).width() as u16;
        frame.render_widget(Line::from(filter), area);
        frame.set_cursor_position((end.min(area.right().saturating_sub(1)), area.y));
        return;
    }
    let line = match (&app.status, app.keymap.pending()) {
        (Some(Status::Error(message)), _) => Line::from(message.as_str().red()),
        (Some(Status::Info(message)), _) => Line::from(message.as_str()),
        (None, Some(key)) => Line::from(key.to_string()).right_aligned(),
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
                format!(" · {count} {tasks}   Esc clears").dim(),
            ])
        }
        (None, None) => Line::default(),
    };
    frame.render_widget(line, area);
}

/// Colors of the sections in list order (inbox first), repeating. Only the 16 terminal
/// colors, so they follow the terminal theme.
const SECTION_COLORS: [Color; 4] = [Color::Blue, Color::Magenta, Color::Green, Color::Yellow];

/// Rules and the preview divider.
const LINE_COLOR: Color = Color::DarkGray;

fn section_color(index: usize) -> Color {
    SECTION_COLORS[index % SECTION_COLORS.len()]
}

/// No box around the list. Sections are rules, tasks get a bullet in the section color,
/// the cursor is a bar in the gutter.
fn render_list(app: &App, frame: &mut Frame, area: Rect, width: usize) {
    let block = list_block();

    let mut items: Vec<ListItem> = Vec::new();
    let mut selected = 0;
    let rows = app.rows();
    for (i, &row) in rows.iter().enumerate() {
        let is_cursor = i == app.cursor;
        let (Row::Section(s) | Row::Task(s, _)) = row;
        let color = section_color(s);

        // Blank line before each section except the first. Not a row, the cursor skips it.
        if i > 0 && matches!(row, Row::Section(_)) {
            items.push(ListItem::new(""));
        }
        if is_cursor {
            selected = items.len();
        }

        // One column gutter for the cursor bar.
        let mut spans = vec![if is_cursor {
            "▌".fg(color)
        } else {
            " ".into()
        }];
        match row {
            Row::Section(s) => {
                let section = &app.sections[s];
                // Only the shown tasks, so with a filter it says how many match.
                let count = rows
                    .iter()
                    .filter(|row| matches!(row, Row::Task(task_s, _) if *task_s == s))
                    .count();
                spans.extend(rule(
                    &section.title(),
                    Some(count),
                    color,
                    width.saturating_sub(1),
                ));
            }
            Row::Task(s, t) => {
                let title = app.sections[s].tasks[t].title();
                spans.push("  ".into());
                spans.push("• ".fg(color));
                let mut style = if is_cursor {
                    Style::new().fg(color)
                } else {
                    Style::new()
                };
                // Listed for a match in the body only: weaker, `…` hints at the preview.
                let body_only = !filter::matches(&title, &app.filter);
                if body_only {
                    style = style.dim();
                }
                spans.extend(highlight(&title, &app.filter, style));
                if body_only {
                    spans.push(" …".dim());
                }
            }
        }

        items.push(ListItem::new(Line::from(spans)));
    }
    if items.is_empty() && !app.filter.is_empty() {
        // Gutter column like the rows, so it lines up with the section rules.
        items.push(ListItem::new(" no matches".dim()));
    }

    let list = List::new(items).block(block);
    // Fresh state each frame: ratatui scrolls just enough to keep the cursor visible.
    let mut state = ListState::default().with_selected(Some(selected));
    frame.render_stateful_widget(list, area, &mut state);
}

/// Space on the right so rules stop before the preview divider.
fn list_block<'a>() -> Block<'a> {
    Block::new().padding(Padding::new(1, 2, 0, 0))
}

/// `── work ─────── 2`, filled to `width`. Section headers and the preview title.
fn rule(title: &str, count: Option<usize>, color: Color, width: usize) -> Vec<Span<'static>> {
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

/// Task body as raw text, read only.
fn render_preview(app: &App, frame: &mut Frame, area: Rect, s: usize, t: usize) {
    let task = &app.sections[s].tasks[t];
    let color = section_color(s);
    let text = body_text(&task.body, &app.filter);

    // No box: a divider on the left, the title as a rule on the first row.
    let divider = Block::new()
        .borders(Borders::LEFT)
        .border_style(Style::new().fg(LINE_COLOR))
        .padding(Padding::horizontal(1));
    let inner = divider.inner(area);
    frame.render_widget(divider, area);

    let [title_area, body_area] =
        Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).areas(inner);
    let title = rule(&task.title(), None, color, title_area.width as usize);
    frame.render_widget(Line::from(title), title_area);
    frame.render_widget(Paragraph::new(text).wrap(Wrap { trim: false }), body_area);
}

/// Markdown stays raw, only heading lines are bold so the structure is easier to scan.
/// Filter matches are highlighted, so a task listed for its body shows why.
fn body_text(body: &str, filter: &str) -> Text<'static> {
    body.lines()
        .map(|line| {
            let style = if line.starts_with('#') {
                Style::new().bold()
            } else {
                Style::new()
            };
            Line::from(highlight(line, filter, style))
        })
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

fn block<'a>(title: String) -> Block<'a> {
    Block::bordered()
        .title(title)
        .title_alignment(Alignment::Center)
        .border_type(BorderType::Rounded)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    use crate::{app::tests::sections, keymap::Action};
    use ratatui::{Terminal, backend::TestBackend, style::Modifier};

    /// Renders the app and returns the screen as text, styles (bold, highlight) are ignored.
    fn screen(app: &App) -> String {
        let mut terminal = Terminal::new(TestBackend::new(50, 9)).unwrap();
        terminal.draw(|frame| render(app, frame)).unwrap();

        let buffer = terminal.backend().buffer();
        buffer
            .content()
            .chunks(buffer.area.width as usize)
            .map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn renders_sections_and_tasks() {
        let app = App::new(
            PathBuf::new(),
            sections(&[(&[], &["Fix typo"]), (&["work"], &["Review PR"])]),
        );
        let screen = screen(&app);
        for text in ["inbox", "• Fix typo", "work", "• Review PR"] {
            assert!(screen.contains(text), "missing {text:?} in\n{screen}");
        }
    }

    #[test]
    fn preview_shows_task_body() {
        let mut app = App::new(PathBuf::new(), sections(&[(&[], &["Fix typo"])]));
        app.sections[0].tasks[0].body = "line one\n## Notes\nline two".to_string();
        app.update(Action::Down);

        let screen = screen(&app);
        for text in [" Fix typo ", "line one", "## Notes", "line two"] {
            assert!(screen.contains(text), "missing {text:?} in\n{screen}");
        }
    }

    #[test]
    fn status_line_shows_errors() {
        let mut app = App::new(PathBuf::new(), sections(&[(&[], &[])]));
        app.update(Action::Undo);
        let screen = screen(&app);
        assert!(
            screen.contains("nothing to undo"),
            "missing status in\n{screen}"
        );
    }

    #[test]
    fn help_lists_bindings() {
        let mut app = App::new(PathBuf::new(), sections(&[(&[], &[])]));
        app.update(Action::Help);
        let mut terminal = Terminal::new(TestBackend::new(60, 20)).unwrap();
        terminal.draw(|frame| render(&app, frame)).unwrap();
        let screen: String = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|c| c.symbol())
            .collect();
        for text in [
            "j / ↓  cursor down",
            "D  delete task (no archive)",
            "q / C-c  quit",
        ] {
            assert!(screen.contains(text), "missing {text:?}");
        }
    }

    #[test]
    fn renamed_popup_lists_renames() {
        let mut app = App::new(PathBuf::new(), sections(&[(&[], &[])]));
        app.renamed = vec![("b.md".into(), "02-b.md".into())];
        let mut terminal = Terminal::new(TestBackend::new(60, 20)).unwrap();
        terminal.draw(|frame| render(&app, frame)).unwrap();
        let screen: String = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|c| c.symbol())
            .collect();
        for text in [" renamed ", "b.md", "-> 02-b.md", "any key closes this"] {
            assert!(screen.contains(text), "missing {text:?}");
        }
    }

    #[test]
    fn filter_input_and_filtered_list() {
        let mut app = App::new(
            PathBuf::new(),
            sections(&[(&[], &["Fix typo"]), (&["work"], &["Review"])]),
        );
        app.update(Action::Filter);
        for c in "rev".chars() {
            app.update(Action::FilterType(c));
        }
        let lines: Vec<String> = screen(&app).lines().map(str::to_string).collect();
        // The first visible section starts at the top, without a blank line.
        assert!(lines[0].contains("work"), "{lines:#?}");
        assert!(lines[1].contains("Review"), "{lines:#?}");
        assert!(!lines.join("\n").contains("Fix typo"));
        assert!(lines[8].starts_with("/rev "), "input at the bottom");

        // Accepted, the filter stays visible in the status line, with its matches.
        app.update(Action::FilterAccept);
        assert!(
            screen(&app)
                .lines()
                .last()
                .unwrap()
                .starts_with("filter: rev · 1 task   Esc clears")
        );
    }

    #[test]
    fn filter_without_matches_says_so() {
        let mut app = App::new(PathBuf::new(), sections(&[(&[], &["Fix typo"])]));
        assert!(!screen(&app).contains("no matches"), "not without a filter");

        app.update(Action::Filter);
        app.update(Action::FilterType('x'));
        app.update(Action::FilterType('q'));
        assert!(screen(&app).lines().next().unwrap().contains("no matches"));
    }

    #[test]
    fn list_highlights_filter_matches() {
        let mut app = App::new(
            PathBuf::new(),
            sections(&[(&[], &["Fix typo", "Buy milk"]), (&["work"], &["Review"])]),
        );
        app.sections[1].tasks[0].body = "fix it".to_string();
        app.update(Action::Filter);
        for c in "fix".chars() {
            app.update(Action::FilterType(c));
        }
        let mut terminal = Terminal::new(TestBackend::new(50, 9)).unwrap();
        terminal.draw(|frame| render(&app, frame)).unwrap();
        let buffer = terminal.backend().buffer();
        let lines: Vec<String> = screen(&app).lines().map(str::to_string).collect();

        assert!(
            lines[0].trim_end().ends_with(" 1"),
            "only shown tasks count"
        );
        // `• Fix typo` starts at column 4, the match in bold yellow, the rest plain.
        assert!(lines[1].contains("• Fix typo"), "{lines:#?}");
        assert_eq!(buffer[(6, 1)].style().fg, Some(Color::Yellow));
        assert!(buffer[(6, 1)].modifier.contains(Modifier::BOLD));
        assert_eq!(buffer[(10, 1)].style().fg, Some(Color::Reset));
        // Matched in its body only: dim, with a hint.
        assert!(lines[4].contains("• Review …"), "{lines:#?}");
        assert!(buffer[(6, 4)].modifier.contains(Modifier::DIM));
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
        let mut app = App::new(PathBuf::new(), sections(&[(&[], &["Fix typo"])]));
        app.sections[0].tasks[0].body = "\n  \n".to_string();

        // On the section header, then on the task with a blank body.
        for _ in 0..2 {
            let screen = screen(&app);
            assert!(!screen.contains('│'), "preview shown in\n{screen}");
            app.update(Action::Down);
        }
    }
}
