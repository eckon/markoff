//! The help (`?`): a panel on the right with all keys, the rest dimmed behind it.

use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Stylize},
    text::Line,
    widgets::{Block, BorderType, Borders, Clear, Padding, Paragraph},
};

use super::{LINE_COLOR, rule, section_color};
use crate::{
    app::Page,
    keymap::{BINDINGS, Binding},
};

/// A panel on the right listing the [`BINDINGS`] of `page`, so it always matches the real keys.
/// Groups as rules like the sections in the list, dots lead from the keys to what they do. The
/// rest is dimmed behind it, the status line stays as it is.
pub fn render(frame: &mut Frame, page: Page) {
    let bindings: Vec<&Binding> = BINDINGS
        .iter()
        .filter(|binding| binding.pages.contains(&page))
        .collect();
    let label = |binding: &Binding| binding.labels().join(" / ");
    let label_width = bindings
        .iter()
        .map(|binding| label(binding).chars().count())
        .max()
        .unwrap_or(0);
    let key_lines: Vec<Line> = bindings
        .iter()
        .map(|binding| {
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
            Line::from(line)
        })
        .collect();
    // Rules as wide as the widest line.
    let rule_width = key_lines.iter().map(Line::width).max().unwrap_or(0);

    let mut lines = vec![Line::from("help".bold())];
    let mut groups = 0;
    for (i, (binding, line)) in bindings.iter().zip(key_lines).enumerate() {
        if i == 0 || bindings[i - 1].group != binding.group {
            lines.push(Line::default());
            lines.push(Line::from(rule(
                None,
                binding.group,
                Vec::new(),
                section_color(groups),
                rule_width,
            )));
            groups += 1;
        }
        lines.push(line);
    }

    let area = frame.area();
    let height = area.height.saturating_sub(1);
    let behind = Rect { height, ..area };
    frame.buffer_mut().set_style(behind, Modifier::DIM);
    let block = Block::new()
        .borders(Borders::LEFT)
        .border_type(BorderType::Double)
        .border_style(LINE_COLOR)
        .padding(Padding::new(2, 1, 1, 0));
    // The columns the border and padding take.
    let chrome = area.width - block.inner(area).width;
    let width = (lines.iter().map(Line::width).max().unwrap_or(0) as u16 + chrome).min(area.width);
    let panel = Rect::new(area.right() - width, area.y, width, height);
    frame.render_widget(Clear, panel);
    frame.render_widget(Paragraph::new(lines).block(block), panel);
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use ratatui::{Terminal, backend::TestBackend, style::Modifier};

    use crate::{
        app::{App, tests::sections},
        keymap::Action,
        ui::render,
    };

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
}
