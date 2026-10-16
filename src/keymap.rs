use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// Everything the user can ask the app to do.
///
/// Input is translated into actions here, so [`crate::app::App::update`] never sees raw keys.
/// That keeps state changes testable without simulating key events.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Up,
    Down,
    Top,
    Bottom,
    PrevSection,
    NextSection,
    MoveUp,
    MoveDown,
    Open,
    NewBelow,
    NewAbove,
    Done,
    Delete,
    Undo,
    Help,
    Quit,
    /// A key that is not mapped on purpose, with a hint what to press instead.
    Hint(&'static str),
}

/// What has to be pressed for a binding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trigger {
    /// A single key without Ctrl.
    Key(KeyCode),
    /// Ctrl plus a letter.
    Ctrl(char),
    /// Shift plus a non-letter key, like `Shift+Down`. Shifted letters are just `Key('J')`.
    Shift(KeyCode),
    /// Two keys in a row, like `gg`.
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
use KeyCode::{Char, Down, End, Enter, Home, Up};
use Trigger::{Ctrl, Key, Seq, Shift};

/// The single source of truth for keys: [`Keymap::handle`] reads it and so does the help
/// overlay, so the two can never disagree. One line per binding, hence no rustfmt.
#[rustfmt::skip]
pub const BINDINGS: &[Binding] = &[
    Binding { triggers: &[Key(Char('j')), Key(Down)], action: A::Down, help: "cursor down" },
    Binding { triggers: &[Key(Char('k')), Key(Up)], action: A::Up, help: "cursor up" },
    Binding { triggers: &[Seq('g', 'g'), Key(Home)], action: A::Top, help: "go to top" },
    Binding { triggers: &[Key(Char('G')), Key(End)], action: A::Bottom, help: "go to bottom" },
    Binding { triggers: &[Key(Char('{'))], action: A::PrevSection, help: "previous section" },
    Binding { triggers: &[Key(Char('}'))], action: A::NextSection, help: "next section" },
    Binding { triggers: &[Key(Char('J')), Shift(Down)], action: A::MoveDown, help: "move task or section down" },
    Binding { triggers: &[Key(Char('K')), Shift(Up)], action: A::MoveUp, help: "move task or section up" },
    Binding { triggers: &[Key(Enter)], action: A::Open, help: "open task in $EDITOR" },
    Binding { triggers: &[Key(Char('o'))], action: A::NewBelow, help: "new task below" },
    Binding { triggers: &[Key(Char('O'))], action: A::NewAbove, help: "new task above" },
    Binding { triggers: &[Key(Char('X'))], action: A::Done, help: "done, move to archive" },
    Binding { triggers: &[Key(Char('D'))], action: A::Delete, help: "delete task (no archive)" },
    Binding { triggers: &[Key(Char('u'))], action: A::Undo, help: "undo" },
    Binding { triggers: &[Key(Char('?'))], action: A::Help, help: "toggle this help" },
    Binding { triggers: &[Key(Char('q')), Ctrl('c')], action: A::Quit, help: "quit" },
];

/// Lowercase keys left unmapped on purpose: the destructive actions are uppercase so they are
/// not hit by accident. Pressing one of these shows the hint instead. Not in the help overlay.
pub const HINTS: &[(char, &str)] = &[
    ('x', "x does nothing, use X to mark done"),
    ('d', "d does nothing, use D to delete"),
];

impl Binding {
    /// Display form of all triggers: `j / ↓`, `gg`, `q / C-c`.
    pub fn label(&self) -> String {
        let label = |trigger: &Trigger| match *trigger {
            Key(Char(c)) => c.to_string(),
            Key(Down) => "↓".to_string(),
            Key(Up) => "↑".to_string(),
            Key(Enter) => "<CR>".to_string(),
            Key(other) => format!("{other:?}"),
            Ctrl(c) => format!("C-{c}"),
            Shift(Down) => "S-↓".to_string(),
            Shift(Up) => "S-↑".to_string(),
            Shift(other) => format!("S-{other:?}"),
            Seq(a, b) => format!("{a}{b}"),
        };
        self.triggers
            .iter()
            .map(label)
            .collect::<Vec<_>>()
            .join(" / ")
    }
}

/// Translates key presses into [`Action`]s, including multi-key sequences like `gg`.
#[derive(Debug, Default)]
pub struct Keymap {
    /// First key of an unfinished sequence, e.g. `g` after the first `g` of `gg`.
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

        // Shift only counts for non-letters: a shifted letter already arrives as `J`.
        let shift = key.modifiers.contains(KeyModifiers::SHIFT) && !matches!(key.code, Char(_));
        for (trigger, action) in triggers {
            match trigger {
                Key(code) if code == key.code && !shift => return Some(action),
                Shift(code) if code == key.code && shift => return Some(action),
                Seq(first, _) if key.code == Char(first) => {
                    self.pending = Some(first);
                    return None;
                }
                _ => {}
            }
        }
        HINTS
            .iter()
            .find(|(c, _)| key.code == Char(*c))
            .map(|(_, hint)| Action::Hint(hint))
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
        assert_eq!(press(&mut keymap, KeyCode::Char('u')), Some(Action::Undo));
        assert_eq!(press(&mut keymap, KeyCode::Char('X')), Some(Action::Done));
        assert_eq!(press(&mut keymap, KeyCode::Char('D')), Some(Action::Delete));
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
                    Shift(code) => keymap.handle(KeyEvent::new(code, KeyModifiers::SHIFT)),
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
        assert_eq!(label(Action::Delete), "D");
        assert_eq!(label(Action::Top), "gg / Home");
        assert_eq!(label(Action::Quit), "q / C-c");
    }

    #[test]
    fn shift_arrow_moves_instead_of_cursor() {
        let mut keymap = Keymap::default();
        let shift_down = KeyEvent::new(Down, KeyModifiers::SHIFT);
        assert_eq!(keymap.handle(shift_down), Some(Action::MoveDown));
        // Terminals send `J` with the shift modifier set, it still is `J`.
        let shift_j = KeyEvent::new(Char('J'), KeyModifiers::SHIFT);
        assert_eq!(keymap.handle(shift_j), Some(Action::MoveDown));
        assert_eq!(press(&mut keymap, Down), Some(Action::Down));
    }

    #[test]
    fn ctrl_does_not_trigger_plain_keys() {
        let mut keymap = Keymap::default();
        let ctrl_j = KeyEvent::new(Char('j'), KeyModifiers::CONTROL);
        assert_eq!(keymap.handle(ctrl_j), None);
    }

    #[test]
    fn gg_is_a_sequence() {
        let mut keymap = Keymap::default();
        assert_eq!(press(&mut keymap, Char('g')), None);
        assert_eq!(keymap.pending(), Some('g'));
        assert_eq!(press(&mut keymap, Char('g')), Some(Action::Top));
        assert_eq!(keymap.pending(), None);
    }

    #[test]
    fn other_key_cancels_sequence() {
        let mut keymap = Keymap::default();
        press(&mut keymap, Char('g'));
        // The second key is swallowed, `gj` neither jumps nor moves.
        assert_eq!(press(&mut keymap, Char('j')), None);
        assert_eq!(keymap.pending(), None);

        press(&mut keymap, Char('g'));
        assert_eq!(press(&mut keymap, KeyCode::Esc), None);
        assert_eq!(keymap.pending(), None);
    }

    #[test]
    fn ctrl_c_quits_during_sequence() {
        let mut keymap = Keymap::default();
        press(&mut keymap, Char('g'));
        let ctrl_c = KeyEvent::new(Char('c'), KeyModifiers::CONTROL);
        assert_eq!(keymap.handle(ctrl_c), Some(Action::Quit));
        assert_eq!(keymap.pending(), None);
    }

    #[test]
    fn lowercase_destructive_keys_only_hint() {
        let mut keymap = Keymap::default();
        assert_eq!(
            press(&mut keymap, Char('x')),
            Some(Action::Hint("x does nothing, use X to mark done"))
        );
        assert_eq!(
            press(&mut keymap, Char('d')),
            Some(Action::Hint("d does nothing, use D to delete"))
        );
    }

    #[test]
    fn unmapped_key() {
        let mut keymap = Keymap::default();
        assert_eq!(press(&mut keymap, KeyCode::Char('z')), None);
        // Esc only cancels, it never quits.
        assert_eq!(press(&mut keymap, KeyCode::Esc), None);
    }
}
