use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// Everything the user can ask the app to do.
///
/// Input is translated into actions here, so [`crate::app::App::update`] never sees raw keys.
/// That keeps state changes testable without simulating key events.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Up,
    Down,
    Open,
    NewBelow,
    NewAbove,
    Done,
    Cut,
    PasteBelow,
    PasteAbove,
    Undo,
    Help,
    Quit,
}

/// What has to be pressed for a binding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trigger {
    /// A single key without Ctrl.
    Key(KeyCode),
    /// Ctrl plus a letter.
    Ctrl(char),
    /// Two keys in a row, like `dd`.
    Seq(char, char),
}

pub struct Binding {
    /// All triggers for this action, e.g. `j` and `Down`.
    pub triggers: &'static [Trigger],
    pub action: Action,
    /// One line for the help overlay.
    pub help: &'static str,
}

use Action as A;
use KeyCode::{Char, Down, Enter, Up};
use Trigger::{Ctrl, Key, Seq};

/// The single source of truth for keys: [`Keymap::handle`] reads it and so does the help
/// overlay, so the two can never disagree. One line per binding, hence no rustfmt.
#[rustfmt::skip]
pub const BINDINGS: &[Binding] = &[
    Binding { triggers: &[Key(Char('j')), Key(Down)], action: A::Down, help: "cursor down" },
    Binding { triggers: &[Key(Char('k')), Key(Up)], action: A::Up, help: "cursor up" },
    Binding { triggers: &[Key(Enter)], action: A::Open, help: "open task in $EDITOR" },
    Binding { triggers: &[Key(Char('o'))], action: A::NewBelow, help: "new task below" },
    Binding { triggers: &[Key(Char('O'))], action: A::NewAbove, help: "new task above" },
    Binding { triggers: &[Key(Char('x'))], action: A::Done, help: "done, move to archive" },
    Binding { triggers: &[Seq('d', 'd')], action: A::Cut, help: "cut task" },
    Binding { triggers: &[Key(Char('p'))], action: A::PasteBelow, help: "paste below" },
    Binding { triggers: &[Key(Char('P'))], action: A::PasteAbove, help: "paste above" },
    Binding { triggers: &[Key(Char('u'))], action: A::Undo, help: "undo" },
    Binding { triggers: &[Key(Char('?'))], action: A::Help, help: "toggle this help" },
    Binding { triggers: &[Key(Char('q')), Ctrl('c')], action: A::Quit, help: "quit" },
];

impl Binding {
    /// Display form of all triggers: `j / ↓`, `dd`, `q / C-c`.
    pub fn label(&self) -> String {
        let label = |trigger: &Trigger| match *trigger {
            Key(Char(c)) => c.to_string(),
            Key(Down) => "↓".to_string(),
            Key(Up) => "↑".to_string(),
            Key(Enter) => "<CR>".to_string(),
            Key(other) => format!("{other:?}"),
            Ctrl(c) => format!("C-{c}"),
            Seq(a, b) => format!("{a}{b}"),
        };
        self.triggers
            .iter()
            .map(label)
            .collect::<Vec<_>>()
            .join(" / ")
    }
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
        let triggers = BINDINGS
            .iter()
            .flat_map(|binding| binding.triggers.iter().map(|t| (*t, binding.action)));
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);

        if ctrl {
            // Ctrl keys work even halfway through a sequence, so `C-c` always quits.
            self.pending = None;
            let KeyCode::Char(c) = key.code else {
                return None;
            };
            return triggers
                .into_iter()
                .find_map(|(t, action)| (t == Ctrl(c)).then_some(action));
        }

        // Any key ends a pending sequence, whether it completes it or not (like vim).
        if let Some(first) = self.pending.take() {
            let KeyCode::Char(second) = key.code else {
                return None;
            };
            return triggers
                .into_iter()
                .find_map(|(t, action)| (t == Seq(first, second)).then_some(action));
        }

        for (trigger, action) in triggers {
            match trigger {
                Key(code) if code == key.code => return Some(action),
                Seq(first, _) if key.code == Char(first) => {
                    self.pending = Some(first);
                    return None;
                }
                _ => {}
            }
        }
        None
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
        assert_eq!(press(&mut keymap, KeyCode::Char('x')), Some(Action::Done));
        assert_eq!(press(&mut keymap, KeyCode::Enter), Some(Action::Open));
        assert_eq!(
            press(&mut keymap, KeyCode::Char('o')),
            Some(Action::NewBelow)
        );
        assert_eq!(
            press(&mut keymap, KeyCode::Char('O')),
            Some(Action::NewAbove)
        );
    }

    #[test]
    fn every_binding_is_reachable() {
        for binding in BINDINGS {
            for trigger in binding.triggers {
                let mut keymap = Keymap::default();
                let action = match *trigger {
                    Key(code) => press(&mut keymap, code),
                    Ctrl(c) => keymap.handle(KeyEvent::new(Char(c), KeyModifiers::CONTROL)),
                    Seq(a, b) => {
                        press(&mut keymap, Char(a));
                        press(&mut keymap, Char(b))
                    }
                };
                assert_eq!(action, Some(binding.action), "trigger {trigger:?}");
            }
        }
    }

    #[test]
    fn labels() {
        let label = |action| {
            BINDINGS
                .iter()
                .find(|b| b.action == action)
                .unwrap()
                .label()
        };
        assert_eq!(label(Action::Down), "j / ↓");
        assert_eq!(label(Action::Cut), "dd");
        assert_eq!(label(Action::Quit), "q / C-c");
    }

    #[test]
    fn ctrl_does_not_trigger_plain_keys() {
        let mut keymap = Keymap::default();
        let ctrl_j = KeyEvent::new(Char('j'), KeyModifiers::CONTROL);
        assert_eq!(keymap.handle(ctrl_j), None);
    }

    #[test]
    fn ctrl_c_quits_during_sequence() {
        let mut keymap = Keymap::default();
        press(&mut keymap, Char('d'));
        let ctrl_c = KeyEvent::new(Char('c'), KeyModifiers::CONTROL);
        assert_eq!(keymap.handle(ctrl_c), Some(Action::Quit));
        assert_eq!(keymap.pending(), None);
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
