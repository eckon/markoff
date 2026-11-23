//! The preview: the body of the selected task next to a list, on both pages.

use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Style, Stylize},
    text::{Line, Text},
    widgets::{Block, Borders, Padding, Paragraph, Wrap},
};

use super::{LINE_COLOR, highlight, rows};

/// A list in `area` drawn by `list`, and next to it the preview of `body` with the matches of
/// `filter`, also when it is empty: the list keeps its width while moving, so wrapped titles
/// stay put. Without a preview (`None`) the list takes all the width, its section headers keep
/// the split width. Returns the list's offset and where the preview went.
pub fn list_with_preview(
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

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use ratatui::style::Color;

    use super::*;
    use crate::{
        app::{App, tests::sections},
        keymap::Action,
        ui::tests::screen,
    };

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
