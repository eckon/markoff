use color_eyre::Result;
use ratatui::{
    DefaultTerminal,
    crossterm::event::{self, Event, KeyEventKind},
};

use crate::{
    keymap::{self, Action},
    task::Section,
    ui,
};

/// One line in the list. The cursor moves over these.
///
/// Section headers are rows too, so the cursor can land on an empty section (to paste into it).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Row {
    Section(usize),
    /// Section index, task index within that section.
    Task(usize, usize),
}

/// Application state.
#[derive(Debug)]
pub struct App {
    running: bool,
    pub sections: Vec<Section>,
    /// Index into [`App::rows`].
    pub cursor: usize,
}

impl App {
    pub fn new(sections: Vec<Section>) -> Self {
        Self {
            running: true,
            sections,
            cursor: 0,
        }
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
            Action::Up => self.cursor = self.cursor.saturating_sub(1),
            Action::Down => {
                let last = self.rows().len().saturating_sub(1);
                self.cursor = (self.cursor + 1).min(last);
            }
            Action::Quit => self.running = false,
        }
    }

    /// All rows in display order: each section header followed by its tasks.
    ///
    /// Computed on demand instead of stored, so it can never be out of sync with `sections`.
    pub fn rows(&self) -> Vec<Row> {
        let mut rows = Vec::new();
        for (s, section) in self.sections.iter().enumerate() {
            rows.push(Row::Section(s));
            rows.extend((0..section.tasks.len()).map(|t| Row::Task(s, t)));
        }
        rows
    }
}

#[cfg(test)]
pub mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::task::Task;

    /// Sections with the given names (empty = inbox) and task titles.
    pub fn sections(spec: &[(&[&str], &[&str])]) -> Vec<Section> {
        spec.iter()
            .map(|(path, tasks)| Section {
                path: path.iter().map(|s| s.to_string()).collect(),
                dir: PathBuf::new(),
                tasks: tasks
                    .iter()
                    .map(|title| Task::parse(PathBuf::new(), &format!("# {title}")))
                    .collect(),
            })
            .collect()
    }

    #[test]
    fn rows_include_section_headers() {
        let app = App::new(sections(&[
            (&[], &["a"]),
            (&["work"], &[]),
            (&["x"], &["b", "c"]),
        ]));
        assert_eq!(
            app.rows(),
            [
                Row::Section(0),
                Row::Task(0, 0),
                Row::Section(1),
                Row::Section(2),
                Row::Task(2, 0),
                Row::Task(2, 1),
            ]
        );
    }

    #[test]
    fn cursor_stays_in_bounds() {
        let mut app = App::new(sections(&[(&[], &["a"])]));
        app.update(Action::Up);
        assert_eq!(app.cursor, 0);
        app.update(Action::Down);
        app.update(Action::Down);
        assert_eq!(app.cursor, 1);
    }

    #[test]
    fn quit_stops_running() {
        let mut app = App::new(sections(&[]));
        app.update(Action::Quit);
        assert!(!app.running);
    }
}
