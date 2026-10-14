use ratatui::{
    Frame,
    layout::Alignment,
    widgets::{Block, BorderType, Paragraph},
};

use crate::app::App;

/// Draws the whole screen from the current state. Rendering never mutates the app.
pub fn render(_app: &App, frame: &mut Frame) {
    let block = Block::bordered()
        .title(" markoff ")
        .title_alignment(Alignment::Center)
        .border_type(BorderType::Rounded);

    let help = Paragraph::new("Press `q`, `Esc` or `Ctrl-C` to quit.")
        .alignment(Alignment::Center)
        .block(block);

    frame.render_widget(help, frame.area());
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{Terminal, backend::TestBackend};

    #[test]
    fn renders_title_and_help() {
        let mut terminal = Terminal::new(TestBackend::new(40, 3)).unwrap();
        terminal.draw(|frame| render(&App::new(), frame)).unwrap();

        let content: String = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect();
        assert!(content.contains("markoff"));
        assert!(content.contains("to quit"));
    }
}
