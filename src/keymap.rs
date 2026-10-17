//! Keys to [`Action`]s. All bindings live in [`BINDINGS`], the help overlay reads it too.

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
    NewSection,
    Done,
    Delete,
    Undo,
    Help,
    Quit,
    /// A key that is not mapped on purpose, with a hint what to press instead.
    Hint(&'static str),
    /// `/`: opens the filter input.
    Filter,
    /// A character typed into the filter input.
    FilterType(char),
    FilterBackspace,
    /// `<CR>` in the filter input: closes it, the filter stays.
    FilterAccept,
    /// `Esc`: closes the filter input and clears the filter.
    FilterClear,
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

/// One line in the help overlay.
pub struct Binding {
    /// Each action with the keys that trigger it. Two for pairs like `j` / `k`.
    pub keys: &'static [(&'static [Trigger], Action)],
    /// Fitting tasks and sections alike.
    pub help: &'static str,
    /// Heading in the help overlay. Bindings of a group stay together in [`BINDINGS`].
    pub group: &'static str,
}

use Action as A;
use KeyCode::{Backspace, Char, Down, End, Enter, Esc, Home, Up};
use Trigger::{Ctrl, Key, Seq, Shift};

/// The single source of truth for keys: [`Keymap::handle`] reads it and so does the help
/// overlay, so the two can never disagree. One line per binding, hence no rustfmt.
#[rustfmt::skip]
pub const BINDINGS: &[Binding] = &[
    Binding { keys: &[(&[Key(Char('j')), Key(Down)], A::Down), (&[Key(Char('k')), Key(Up)], A::Up)], help: "down / up", group: "move" },
    Binding { keys: &[(&[Seq('g', 'g'), Key(Home)], A::Top), (&[Key(Char('G')), Key(End)], A::Bottom)], help: "top / bottom", group: "move" },
    Binding { keys: &[(&[Key(Char('{'))], A::PrevSection), (&[Key(Char('}'))], A::NextSection)], help: "previous / next section", group: "move" },
    Binding { keys: &[(&[Key(Char('J')), Shift(Down)], A::MoveDown), (&[Key(Char('K')), Shift(Up)], A::MoveUp)], help: "move down / up", group: "change" },
    Binding { keys: &[(&[Key(Enter)], A::Open)], help: "open in $EDITOR", group: "change" },
    Binding { keys: &[(&[Key(Char('o'))], A::NewBelow), (&[Key(Char('O'))], A::NewAbove)], help: "new task below / above", group: "change" },
    Binding { keys: &[(&[Key(Char('N'))], A::NewSection)], help: "new section", group: "change" },
    Binding { keys: &[(&[Key(Char('X'))], A::Done)], help: "done (archive)", group: "change" },
    Binding { keys: &[(&[Key(Char('D'))], A::Delete)], help: "delete", group: "change" },
    Binding { keys: &[(&[Key(Char('u'))], A::Undo)], help: "undo", group: "change" },
    Binding { keys: &[(&[Key(Char('/'))], A::Filter)], help: "filter", group: "other" },
    Binding { keys: &[(&[Key(Esc)], A::FilterClear)], help: "clear filter / close help", group: "other" },
    Binding { keys: &[(&[Key(Char('?'))], A::Help)], help: "toggle help", group: "other" },
    Binding { keys: &[(&[Key(Char('q')), Ctrl('c')], A::Quit)], help: "quit", group: "other" },
];

/// Lowercase keys left unmapped on purpose: the destructive actions are uppercase so they are
/// not hit by accident. Pressing one of these shows the hint instead. Not in the help overlay.
const HINTS: &[(char, &str)] = &[
    ('x', "x does nothing, use X to mark done"),
    ('d', "d does nothing, use D to delete"),
];

impl Binding {
    /// Display form of the triggers: `j` and `k`, `gg`, `q` and `C-c`. Arrows, `Home` / `End`
    /// and shifted arrows only clutter the help, the README lists them.
    pub fn labels(&self) -> Vec<String> {
        self.triggers()
            .map(|(trigger, _)| trigger)
            .filter(|trigger| !matches!(trigger, Key(Up | Down | Home | End) | Shift(_)))
            .map(|trigger| trigger.label())
            .collect()
    }

    fn triggers(&self) -> impl Iterator<Item = (Trigger, Action)> {
        self.keys
            .iter()
            .flat_map(|(triggers, action)| triggers.iter().map(|t| (*t, *action)))
    }
}

impl Trigger {
    fn label(&self) -> String {
        match *self {
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
        }
    }
}

/// Translates key presses into [`Action`]s, including multi-key sequences like `gg`.
#[derive(Debug, Default)]
pub struct Keymap {
    /// First key of an unfinished sequence, e.g. `g` after the first `g` of `gg`.
    pending: Option<char>,
}

impl Keymap {
    /// Returns `None` while a sequence is incomplete and for unmapped keys. `Esc` during a
    /// sequence only cancels it.
    pub fn handle(&mut self, key: KeyEvent) -> Option<Action> {
        let triggers = BINDINGS.iter().flat_map(Binding::triggers);
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

/// Key hints in the status line while the filter input is open, see [`filter_input`].
pub const FILTER_INPUT_HINTS: &[(&str, &str)] = &[("<CR>", "keep filter"), ("Esc", "clear filter")];

/// Keys while the filter input is open: text goes into the filter, not to [`BINDINGS`].
/// Not in the help, the README lists them. `C-c` still quits.
pub fn filter_input(key: KeyEvent) -> Option<Action> {
    if key.modifiers.contains(KeyModifiers::CONTROL) {
        return (key.code == Char('c')).then_some(Action::Quit);
    }
    match key.code {
        Char(c) => Some(Action::FilterType(c)),
        Backspace => Some(Action::FilterBackspace),
        Enter => Some(Action::FilterAccept),
        Esc => Some(Action::FilterClear),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

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
        assert_eq!(
            press(&mut keymap, KeyCode::Char('N')),
            Some(Action::NewSection)
        );
    }

    #[test]
    fn every_binding_is_reachable() {
        for (trigger, expected) in BINDINGS.iter().flat_map(Binding::triggers) {
            let mut keymap = Keymap::default();
            let action = match trigger {
                Key(code) => press(&mut keymap, code),
                Ctrl(c) => keymap.handle(KeyEvent::new(Char(c), KeyModifiers::CONTROL)),
                Shift(code) => keymap.handle(KeyEvent::new(code, KeyModifiers::SHIFT)),
                Seq(a, b) => {
                    press(&mut keymap, Char(a));
                    press(&mut keymap, Char(b))
                }
            };
            assert_eq!(action, Some(expected), "trigger {trigger:?}");
        }
    }

    #[test]
    fn labels() {
        let labels = |help| BINDINGS.iter().find(|b| b.help == help).unwrap().labels();
        assert_eq!(labels("down / up"), ["j", "k"]);
        assert_eq!(labels("delete"), ["D"]);
        assert_eq!(labels("top / bottom"), ["gg", "G"]);
        assert_eq!(labels("move down / up"), ["J", "K"]);
        assert_eq!(labels("quit"), ["q", "C-c"]);
    }

    /// AGENTS.md: a key change updates `BINDINGS` and the README table together. Only the keys
    /// are compared, README may group them and word the actions differently.
    #[test]
    fn readme_lists_the_same_keys() {
        let readme = include_str!("../README.md");
        let table = readme
            .split("## Keys")
            .nth(1)
            .expect("README has a Keys section");
        let readme_keys: BTreeSet<String> = table
            .lines()
            .skip_while(|line| !line.starts_with('|'))
            .take_while(|line| line.starts_with('|'))
            // Header and separator row.
            .skip(2)
            .filter_map(|line| line.split('|').nth(1))
            .flat_map(|keys| keys.split(" / "))
            .map(|key| key.trim().trim_matches('`').to_string())
            .collect();
        let bindings: BTreeSet<String> = BINDINGS
            .iter()
            .flat_map(Binding::triggers)
            .map(|(trigger, _)| trigger.label())
            .collect();
        assert_eq!(readme_keys, bindings);
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
    }

    #[test]
    fn esc_clears_filter_and_never_quits() {
        let mut keymap = Keymap::default();
        assert_eq!(press(&mut keymap, KeyCode::Esc), Some(Action::FilterClear));
    }

    #[test]
    fn filter_input_takes_text() {
        let key = |code| filter_input(KeyEvent::from(code));
        // Keys that are bindings elsewhere are text here.
        assert_eq!(key(Char('j')), Some(Action::FilterType('j')));
        assert_eq!(key(Char('?')), Some(Action::FilterType('?')));
        assert_eq!(key(Backspace), Some(Action::FilterBackspace));
        assert_eq!(key(Enter), Some(Action::FilterAccept));
        assert_eq!(key(Esc), Some(Action::FilterClear));
        assert_eq!(key(Down), None);
        let ctrl = |c| filter_input(KeyEvent::new(Char(c), KeyModifiers::CONTROL));
        assert_eq!(ctrl('c'), Some(Action::Quit));
        assert_eq!(ctrl('j'), None);
    }
}
