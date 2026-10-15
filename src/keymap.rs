use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// Everything the user can ask the app to do.
///
/// Input is translated into actions here, so [`crate::app::App::update`] never sees raw keys.
/// That keeps state changes testable without simulating key events.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Up,
    Down,
    Quit,
}

/// Translates a key press into an [`Action`].
///
/// This is the place for multi-key sequences (`gg`, `dd`, counts) later: turn it into a
/// struct that holds the pending keys and returns `None` while a sequence is incomplete.
pub fn map_key(key: KeyEvent) -> Option<Action> {
    match key.code {
        KeyCode::Char('j') | KeyCode::Down => Some(Action::Down),
        KeyCode::Char('k') | KeyCode::Up => Some(Action::Up),
        KeyCode::Char('q') => Some(Action::Quit),
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
        assert_eq!(
            map_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)),
            Some(Action::Quit)
        );
    }

    #[test]
    fn movement_keys() {
        assert_eq!(
            map_key(KeyEvent::from(KeyCode::Char('j'))),
            Some(Action::Down)
        );
        assert_eq!(map_key(KeyEvent::from(KeyCode::Down)), Some(Action::Down));
        assert_eq!(
            map_key(KeyEvent::from(KeyCode::Char('k'))),
            Some(Action::Up)
        );
        assert_eq!(map_key(KeyEvent::from(KeyCode::Up)), Some(Action::Up));
    }

    #[test]
    fn unmapped_key() {
        assert_eq!(map_key(KeyEvent::from(KeyCode::Char('z'))), None);
        // Esc only cancels, it never quits.
        assert_eq!(map_key(KeyEvent::from(KeyCode::Esc)), None);
    }
}
