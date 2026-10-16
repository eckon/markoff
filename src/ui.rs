use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Color, Style, Stylize},
    text::{Line, Span, Text},
    widgets::{
        Block, BorderType, Borders, Clear, List, ListItem, ListState, Padding, Paragraph, Wrap,
    },
};

use crate::{
    app::{App, Row, Status},
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
    let width = labels.iter().map(|l| l.chars().count()).max().unwrap_or(0);

    let mut lines: Vec<Line> = BINDINGS
        .iter()
        .zip(&labels)
        .map(|(binding, label)| {
            Line::from(vec![
                format!("{label:>width$}  ").bold(),
                binding.help.into(),
            ])
        })
        .collect();
    lines.push(Line::default());
    lines.push(Line::from("any key closes this help").dim().centered());

    let height = lines.len() as u16 + 2;
    let area = frame
        .area()
        .centered(Constraint::Length(44), Constraint::Length(height));
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(lines).block(block(" help ".to_string())),
        area,
    );
}

/// Last error or message, otherwise the pending key of an unfinished sequence (`d`).
fn render_status(app: &App, frame: &mut Frame, area: Rect) {
    let line = match (&app.status, app.keymap.pending()) {
        (Some(Status::Error(message)), _) => Line::from(message.as_str().red()),
        (Some(Status::Info(message)), _) => Line::from(message.as_str()),
        (None, Some(key)) => Line::from(key.to_string()).right_aligned(),
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
    for (i, row) in app.rows().into_iter().enumerate() {
        let is_cursor = i == app.cursor;
        let (Row::Section(s) | Row::Task(s, _)) = row;
        let color = section_color(s);

        // Blank line before each section except the first. Not a row, the cursor skips it.
        if s > 0 && matches!(row, Row::Section(_)) {
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
                let count = section.tasks.len();
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
                spans.push(if is_cursor {
                    title.fg(color)
                } else {
                    title.into()
                });
            }
        }

        items.push(ListItem::new(Line::from(spans)));
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
    let text = body_text(&task.body);

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
fn body_text(body: &str) -> Text<'_> {
    body.lines()
        .map(|line| {
            if line.starts_with('#') {
                Line::from(line.bold())
            } else {
                Line::from(line)
            }
        })
        .collect()
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
    use ratatui::{Terminal, backend::TestBackend};

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
