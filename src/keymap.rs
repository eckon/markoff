use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// Everything the user can ask the app to do.
///
/// Input is translated into actions here, so [`crate::app::App::update`] never sees raw keys.
/// That keeps state changes testable without simulating key events.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Up,
    Down,
    Cut,
    PasteBelow,
    PasteAbove,
    Undo,
    Quit,
}

/// Translates key presses into [`Action`]s, including multi-key sequences like `dd`.
#[derive(Debug, Default)]
pub struct Keymap {
    /// First key of an unfinished sequence, e.g. `d` after the first `d` of `dd`.
    pending: Option<char>,
}

impl Keymap {
    /// Returns `None` while a sequence is incomplete, for `Esc` and for unmapped keys.
    pub fn handle(&mut self, key: KeyEvent) -> Option<Action> {
        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            return Some(Action::Quit);
        }

        // Any key ends a pending sequence, whether it completes it or not (like vim).
        if let Some(first) = self.pending.take() {
            return match (first, key.code) {
                ('d', KeyCode::Char('d')) => Some(Action::Cut),
                _ => None,
            };
        }

        match key.code {
            KeyCode::Char('j') | KeyCode::Down => Some(Action::Down),
            KeyCode::Char('k') | KeyCode::Up => Some(Action::Up),
            KeyCode::Char('d') => {
                self.pending = Some('d');
                None
            }
            KeyCode::Char('p') => Some(Action::PasteBelow),
            KeyCode::Char('P') => Some(Action::PasteAbove),
            KeyCode::Char('u') => Some(Action::Undo),
            KeyCode::Char('q') => Some(Action::Quit),
            _ => None,
        }
    }

    /// The unfinished sequence, to show in the status line.
    pub fn pending(&self) -> Option<char> {
        self.pending
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn press(keymap: &mut Keymap, code: KeyCode) -> Option<Action> {
        keymap.handle(KeyEvent::from(code))
    }

    #[test]
    fn quit_keys() {
        let mut keymap = Keymap::default();
        assert_eq!(press(&mut keymap, KeyCode::Char('q')), Some(Action::Quit));
        let ctrl_c = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
        assert_eq!(keymap.handle(ctrl_c), Some(Action::Quit));
    }

    #[test]
    fn single_keys() {
        let mut keymap = Keymap::default();
        assert_eq!(press(&mut keymap, KeyCode::Char('j')), Some(Action::Down));
        assert_eq!(press(&mut keymap, KeyCode::Down), Some(Action::Down));
        assert_eq!(press(&mut keymap, KeyCode::Char('k')), Some(Action::Up));
        assert_eq!(press(&mut keymap, KeyCode::Up), Some(Action::Up));
        assert_eq!(
            press(&mut keymap, KeyCode::Char('p')),
            Some(Action::PasteBelow)
        );
        assert_eq!(
            press(&mut keymap, KeyCode::Char('P')),
            Some(Action::PasteAbove)
        );
        assert_eq!(press(&mut keymap, KeyCode::Char('u')), Some(Action::Undo));
    }

    #[test]
    fn dd_is_a_sequence() {
        let mut keymap = Keymap::default();
        assert_eq!(press(&mut keymap, KeyCode::Char('d')), None);
        assert_eq!(keymap.pending(), Some('d'));
        assert_eq!(press(&mut keymap, KeyCode::Char('d')), Some(Action::Cut));
        assert_eq!(keymap.pending(), None);
    }

    #[test]
    fn other_key_cancels_sequence() {
        let mut keymap = Keymap::default();
        press(&mut keymap, KeyCode::Char('d'));
        // The second key is swallowed, `dj` neither cuts nor moves.
        assert_eq!(press(&mut keymap, KeyCode::Char('j')), None);
        assert_eq!(keymap.pending(), None);

        press(&mut keymap, KeyCode::Char('d'));
        assert_eq!(press(&mut keymap, KeyCode::Esc), None);
        assert_eq!(keymap.pending(), None);
    }

    #[test]
    fn unmapped_key() {
        let mut keymap = Keymap::default();
        assert_eq!(press(&mut keymap, KeyCode::Char('z')), None);
        // Esc only cancels, it never quits.
        assert_eq!(press(&mut keymap, KeyCode::Esc), None);
    }
}
