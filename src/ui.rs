use ratatui::{
    Frame,
    layout::Alignment,
    style::{Modifier, Style, Stylize},
    widgets::{Block, BorderType, List, ListItem, ListState},
};

use crate::app::{App, Row};

/// Draws the whole screen from the current state. Rendering never mutates the app.
pub fn render(app: &App, frame: &mut Frame) {
    let block = Block::bordered()
        .title(" markoff ")
        .title_alignment(Alignment::Center)
        .border_type(BorderType::Rounded);

    let items: Vec<ListItem> = app
        .rows()
        .into_iter()
        .map(|row| match row {
            Row::Section(s) => ListItem::new(app.sections[s].title().bold()),
            Row::Task(s, t) => ListItem::new(format!("  {}", app.sections[s].tasks[t].title())),
        })
        .collect();

    let list = List::new(items)
        .block(block)
        .highlight_style(Style::new().add_modifier(Modifier::REVERSED));

    // Fresh state each frame: ratatui scrolls just enough to keep the cursor visible.
    let mut state = ListState::default().with_selected(Some(app.cursor));
    frame.render_stateful_widget(list, frame.area(), &mut state);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::tests::sections;
    use ratatui::{Terminal, backend::TestBackend};

    #[test]
    fn renders_sections_and_tasks() {
        let app = App::new(sections(&[
            (&[], &["Fix typo"]),
            (&["work"], &["Review PR"]),
        ]));
        let mut terminal = Terminal::new(TestBackend::new(20, 6)).unwrap();
        terminal.draw(|frame| render(&app, frame)).unwrap();

        // Text only, styles (bold, highlight) are not part of this test.
        let buffer = terminal.backend().buffer();
        let lines: Vec<String> = buffer
            .content()
            .chunks(buffer.area.width as usize)
            .map(|row| row.iter().map(|cell| cell.symbol()).collect())
            .collect();
        assert_eq!(
            lines,
            [
                "╭──── markoff ─────╮",
                "│inbox             │",
                "│  Fix typo        │",
                "│work              │",
                "│  Review PR       │",
                "╰──────────────────╯",
            ]
        );
    }
}
