//! The help (`?`): an overlay on the right with all keys, the rest dimmed behind it. It scrolls
//! and has a filter of its own, both gone when it closes.

use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style, Stylize},
    text::Line,
    widgets::{Block, BorderType, Borders, Clear, Padding, Paragraph},
};

use super::{LINE_COLOR, filled_line, highlight, rows::more, section_color};
use crate::{
    app::Page,
    filter,
    keymap::{BINDINGS, Binding, Context},
};

/// The bindings the help lists on `page`: the page's, then the help's own, those of both only
/// once. Only those whose keys or text match `filter`.
pub fn bindings(page: Page, filter: &str) -> Vec<&'static Binding> {
    let page = Context::Page(page);
    (BINDINGS.iter())
        .filter(|binding| binding.contexts.contains(&page))
        .chain(BINDINGS.iter().filter(|binding| {
            binding.contexts.contains(&Context::Help) && !binding.contexts.contains(&page)
        }))
        .filter(|binding| {
            let keys = [binding.labels(), binding.second_labels()].concat();
            filter::matches(binding.help, filter)
                || keys.iter().any(|key| filter::matches(key, filter))
        })
        .collect()
}

/// An overlay on the right listing the [`bindings`] of `page`, so it always matches the real keys,
/// with the matches of `filter`. Group headers like the section headers in the list, dots lead
/// from the first keys to what they do, the other keys follow. The rest is dimmed behind it, the
/// footer stays as it is. Starts from line `offset`, returns it kept within the lines, like the
/// lists with what is out of view counted above and below.
pub fn render(frame: &mut Frame, page: Page, filter: &str, offset: usize) -> usize {
    let label = |binding: &Binding| binding.labels().join(" / ");
    // Widths from all bindings, so the help keeps its width while filtering.
    let all = bindings(page, "");
    let label_width = (all.iter())
        .map(|binding| label(binding).chars().count())
        .max()
        .unwrap_or(0);
    // Only the keys are bold, so the `/` between them is not taken for the filter key.
    let keys = |labels: Vec<String>| {
        let mut spans = Vec::new();
        for key in labels {
            if !spans.is_empty() {
                spans.push(" / ".dim());
            }
            spans.extend(highlight(&key, filter, Style::new().bold()));
        }
        spans
    };
    let key_line = |binding: &Binding| {
        let mut line = keys(binding.labels());
        let dots = "·".repeat(label_width + 2 - label(binding).chars().count());
        line.push(format!(" {dots} ").fg(LINE_COLOR));
        line.extend(highlight(binding.help, filter, Style::new()));
        // The keys for people outside vim after the text, they would widen the key column.
        let second = binding.second_labels();
        if !second.is_empty() {
            line.push(", also ".into());
            line.extend(keys(second));
        }
        Line::from(line)
    };
    // Rules as wide as the widest line.
    let rule_width = all.iter().map(|b| key_line(b).width()).max().unwrap_or(0);

    let mut lines = vec![Line::from(vec![
        "help".into(),
        " · ".dim(),
        page.name().into(),
    ])];
    let bindings = bindings(page, filter);
    if bindings.is_empty() {
        lines.extend([Line::default(), "no matches".dim().into()]);
    }
    // Which lines are keys, for the counts of those out of view.
    let mut keys_at = Vec::new();
    let mut groups = 0;
    for (i, binding) in bindings.iter().enumerate() {
        if i == 0 || bindings[i - 1].group != binding.group {
            lines.push(Line::default());
            lines.push(Line::from(filled_line(
                None,
                binding.group.fg(section_color(groups)).bold(),
                Vec::new(),
                rule_width,
            )));
            groups += 1;
        }
        keys_at.push(lines.len());
        lines.push(key_line(binding));
    }

    let area = frame.area();
    let height = area.height.saturating_sub(1);
    let behind = Rect { height, ..area };
    frame.buffer_mut().set_style(behind, Modifier::DIM);
    let block = Block::new()
        .borders(Borders::LEFT)
        .border_type(BorderType::Double)
        .border_style(LINE_COLOR)
        .padding(Padding::new(2, 1, 1, 1));
    // The columns the border and padding take.
    let chrome = area.width - block.inner(area).width;
    let width = (rule_width as u16 + chrome).min(area.width);
    let overlay = Rect::new(area.right() - width, area.y, width, height);
    frame.render_widget(Clear, overlay);
    let inner = block.inner(overlay);
    let offset = offset.min(lines.len().saturating_sub(inner.height as usize));
    let end = offset + inner.height as usize;
    // On the padding lines above and below.
    let hints = [
        (inner.y.checked_sub(1), "↑", 0..offset),
        (Some(inner.bottom()), "↓", end..lines.len()),
    ];
    frame.render_widget(
        Paragraph::new(lines)
            .block(block)
            .scroll((offset as u16, 0)),
        overlay,
    );
    for (y, arrow, out_of_view) in hints {
        let Some(y) = y.filter(|&y| !out_of_view.is_empty() && y < overlay.bottom()) else {
            continue;
        };
        let count = keys_at.iter().filter(|i| out_of_view.contains(i)).count();
        let text = format!("{arrow} {}", more(count));
        frame
            .buffer_mut()
            .set_span(inner.x, y, &text.fg(LINE_COLOR), inner.width);
    }
    offset
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use ratatui::{
        Terminal,
        backend::TestBackend,
        style::{Color, Modifier},
    };

    use crate::{
        app::{App, tests::sections},
        keymap::Action,
        ui::render,
    };

    #[test]
    fn help_lists_bindings() {
        let mut app = App::new(PathBuf::new(), sections(&[("home", &[])]));
        app.update(Action::Help);
        let mut terminal = Terminal::new(TestBackend::new(80, 30)).unwrap();
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
            "help · tasks",
            "── move ──",
            "j / k ········ down / up, also ↓ / ↑",
            "o / O ········ new task below / above",
            "D ············ delete task, or empty section",
            "X ············ done: archive, a repeating task hides",
            "q ············ quit, also C-c",
            "── show ──",
            "── global ──",
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

        // The list behind the overlay is dimmed, the footer is not.
        assert!(modifier("home", 0).contains(Modifier::DIM));
        assert_eq!(modifier("? toggle help", 0), Modifier::BOLD);
    }

    #[test]
    fn help_scrolls_and_counts_keys_out_of_view() {
        let mut app = App::new(PathBuf::new(), sections(&[("home", &[])]));
        app.update(Action::Help);
        let mut terminal = Terminal::new(TestBackend::new(80, 20)).unwrap();
        let mut draw = |app: &mut App| {
            let drawn = terminal.draw(|frame| app.help_offset = render(app, frame).help_offset);
            let screen: String = (drawn.unwrap().buffer.content().iter())
                .map(|c| c.symbol())
                .collect();
            screen
        };
        let screen = draw(&mut app);
        assert!(screen.contains("help · tasks"));
        assert!(screen.contains("↓ 10 more"), "{screen}");
        assert_eq!(screen.matches("more").count(), 1, "nothing above");

        // Past the end is cut back to the last line at the bottom.
        app.update(Action::Bottom);
        let screen = draw(&mut app);
        assert_eq!(app.help_offset, 15);
        assert!(screen.contains("↑ 10 more"), "{screen}");
        assert!(screen.contains("? ············ close help, also Esc"));
        assert_eq!(screen.matches("more").count(), 1, "nothing below");
    }

    #[test]
    fn help_filters_keys_and_texts() {
        let mut app = App::new(PathBuf::new(), sections(&[("home", &[])]));
        app.update(Action::Help);
        let mut terminal = Terminal::new(TestBackend::new(80, 20)).unwrap();
        let mut draw = |app: &App| {
            let drawn = terminal.draw(|frame| {
                render(app, frame);
            });
            let buffer = drawn.unwrap().buffer.clone();
            let screen: String = buffer.content().iter().map(|c| c.symbol()).collect();
            (screen, buffer)
        };
        // By key, the group without a match drops out.
        app.help_filter = "C-c".to_string();
        let (screen, buffer) = draw(&app);
        assert!(screen.contains("q ············ quit, also C-c"), "{screen}");
        assert!(!screen.contains("── move"));
        let at = screen[..screen.find("C-c").unwrap()].chars().count();
        assert_eq!(
            buffer.content()[at].fg,
            Color::Yellow,
            "the match is highlighted"
        );

        // By text.
        app.help_filter = "SECTION".to_string();
        let (screen, _) = draw(&app);
        assert!(screen.contains("{ / } ········ previous / next section"));
        assert!(screen.contains("N ············ new section"));
        assert!(!screen.contains("quit"));

        app.help_filter = "zz".to_string();
        let (screen, _) = draw(&app);
        assert!(screen.contains("no matches"));
    }
}
