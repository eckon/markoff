use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Modifier, Style, Stylize},
    text::{Line, Text},
    widgets::{Block, BorderType, Clear, List, ListItem, ListState, Paragraph, Wrap},
};

use crate::{
    app::{App, Row},
    keymap::{BINDINGS, Binding},
};

/// Draws the whole screen from the current state. Rendering never mutates the app.
pub fn render(app: &App, frame: &mut Frame) {
    let [main_area, status_area] =
        Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).areas(frame.area());
    let [list_area, preview_area] =
        Layout::horizontal([Constraint::Percentage(40), Constraint::Percentage(60)])
            .areas(main_area);

    render_list(app, frame, list_area);
    render_preview(app, frame, preview_area);
    render_status(app, frame, status_area);
    if app.show_help {
        render_help(frame);
    }
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
        (Some(status), _) => Line::from(status.as_str().red()),
        (None, Some(key)) => Line::from(key.to_string()).right_aligned(),
        (None, None) => Line::default(),
    };
    frame.render_widget(line, area);
}

fn render_list(app: &App, frame: &mut Frame, area: Rect) {
    let items: Vec<ListItem> = app
        .rows()
        .into_iter()
        .map(|row| match row {
            Row::Section(s) => ListItem::new(app.sections[s].title().bold()),
            Row::Task(s, t) => ListItem::new(format!("  {}", app.sections[s].tasks[t].title())),
        })
        .collect();

    let list = List::new(items)
        .block(block(" markoff ".to_string()))
        .highlight_style(Style::new().add_modifier(Modifier::REVERSED));

    // Fresh state each frame: ratatui scrolls just enough to keep the cursor visible.
    let mut state = ListState::default().with_selected(Some(app.cursor));
    frame.render_stateful_widget(list, area, &mut state);
}

/// Task body as raw text, read only. On a section header: name and task count.
fn render_preview(app: &App, frame: &mut Frame, area: Rect) {
    let (title, text) = match app.selected() {
        Some(Row::Task(s, t)) => {
            let task = &app.sections[s].tasks[t];
            (task.title(), body_text(&task.body))
        }
        Some(Row::Section(s)) => {
            let section = &app.sections[s];
            let count = section.tasks.len();
            let noun = if count == 1 { "task" } else { "tasks" };
            (section.title(), Text::from(format!("{count} {noun}")))
        }
        None => (String::new(), Text::default()),
    };

    let preview = Paragraph::new(text)
        .block(block(format!(" {title} ")))
        .wrap(Wrap { trim: false });
    frame.render_widget(preview, area);
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
        for text in ["inbox", "  Fix typo", "work", "  Review PR"] {
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
        app.update(Action::PasteBelow);
        let screen = screen(&app);
        assert!(
            screen.contains("nothing to paste"),
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
        for text in ["j / ↓  cursor down", "dd  cut task", "q / C-c  quit"] {
            assert!(screen.contains(text), "missing {text:?}");
        }
    }

    #[test]
    fn preview_shows_section_count() {
        let app = App::new(PathBuf::new(), sections(&[(&[], &["a", "b"])]));
        let screen = screen(&app);
        assert!(screen.contains("2 tasks"), "missing count in\n{screen}");
    }
}
