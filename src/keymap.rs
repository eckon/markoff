use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// Everything the user can ask the app to do.
///
/// Input is translated into actions here, so [`crate::app::App::update`] never sees raw keys.
/// That keeps state changes testable without simulating key events.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Quit,
}

/// Translates a key press into an [`Action`].
///
/// This is the place for multi-key sequences (`gg`, `dd`, counts) later: turn it into a
/// struct that holds the pending keys and returns `None` while a sequence is incomplete.
pub fn map_key(key: KeyEvent) -> Option<Action> {
    match key.code {
        KeyCode::Char('q') | KeyCode::Esc => Some(Action::Quit),
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => Some(Action::Quit),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quit_keys() {
        assert_eq!(
            map_key(KeyEvent::from(KeyCode::Char('q'))),
            Some(Action::Quit)
        );
        assert_eq!(map_key(KeyEvent::from(KeyCode::Esc)), Some(Action::Quit));
        assert_eq!(
            map_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)),
            Some(Action::Quit)
        );
    }

    #[test]
    fn unmapped_key() {
        assert_eq!(map_key(KeyEvent::from(KeyCode::Char('x'))), None);
    }
}
