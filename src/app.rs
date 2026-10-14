use color_eyre::Result;
use ratatui::{
    DefaultTerminal,
    crossterm::event::{self, Event, KeyEventKind},
};

use crate::{
    keymap::{self, Action},
    ui,
};

/// Application state.
#[derive(Debug, Default)]
pub struct App {
    running: bool,
}

impl App {
    pub fn new() -> Self {
        Self { running: true }
    }

    /// Main loop: draw, wait for input, update.
    ///
    /// Events are read on this thread (no background reader), so the TUI can hand the
    /// terminal to `$EDITOR` with `ratatui::restore()` and take it back with
    /// `ratatui::init()` without the two fighting over stdin.
    pub fn run(mut self, terminal: &mut DefaultTerminal) -> Result<()> {
        while self.running {
            terminal.draw(|frame| ui::render(&self, frame))?;
            self.handle_event(event::read()?);
        }
        Ok(())
    }

    fn handle_event(&mut self, event: Event) {
        // Only presses: some terminals also report key releases.
        if let Event::Key(key) = event
            && key.kind == KeyEventKind::Press
            && let Some(action) = keymap::map_key(key)
        {
            self.update(action);
        }
    }

    pub fn update(&mut self, action: Action) {
        match action {
            Action::Quit => self.running = false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quit_stops_running() {
        let mut app = App::new();
        app.update(Action::Quit);
        assert!(!app.running);
    }
}
