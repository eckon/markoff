//! Keys to [`Action`]s. All bindings live in [`BINDINGS`], the help overlay reads it too.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::app::Page;

/// Everything the user can ask the app to do.
///
/// Input is translated into actions here, so [`crate::app::App::update`] never sees raw keys.
/// That keeps state changes testable without simulating key events.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Up,
    Down,
    /// `h` / `l` on the archive page: a heatmap column back or forward.
    Left,
    Right,
    /// `H` / `J` / `K` / `L` on the archive page: like `h` / `j` / `k` / `l` to the next cell
    /// with tasks done.
    SkipLeft,
    SkipDown,
    SkipUp,
    SkipRight,
    /// `Tab` on the archive page: the heatmap or the list takes the move keys.
    Focus,
    /// `p`: shows or hides the preview of the task at the cursor, on each page of its own.
    Preview,
    Top,
    Bottom,
    PrevSection,
    NextSection,
    /// `1`-`9`: the first task of the section with this index, its header if it is empty.
    JumpSection(usize),
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
    /// `[` / `]`: the page before or after the current one, see [`Page`].
    PrevPage,
    NextPage,
    Quit,
    /// `/`: opens the filter input.
    Filter,
    /// A character typed into the filter input.
    FilterType(char),
    FilterBackspace,
    /// `C-w` in the filter input: deletes the last word, like in a shell.
    FilterDeleteWord,
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
    /// Where the keys work. The help shows only those of the current page.
    pub pages: &'static [Page],
}

/// Bindings of the todo list only.
const TODO: &[Page] = &[Page::Todo];

/// Bindings of the archive page only.
const ARCHIVE: &[Page] = &[Page::Archive];

/// Bindings that work on every page.
const ALL: &[Page] = &[Page::Todo, Page::Archive];

use Action as A;
use KeyCode::{Backspace, Char, Down, End, Enter, Esc, Home, Left, Right, Tab, Up};
use Trigger::{Ctrl, Key, Seq, Shift};

/// The single source of truth for keys: [`Keymap::handle`] reads it and so does the help
/// overlay, so the two can never disagree. A key may be in several groups of a page, like `j` in
/// the archive's heatmap and list, it does the same there. One line per binding, hence no
/// rustfmt.
#[rustfmt::skip]
pub const BINDINGS: &[Binding] = &[
    Binding { keys: &[(&[Key(Char('j')), Key(Down)], A::Down), (&[Key(Char('k')), Key(Up)], A::Up)], help: "down / up", group: "move", pages: TODO },
    Binding { keys: &[(&[Seq('g', 'g'), Key(Home)], A::Top), (&[Key(Char('G')), Key(End)], A::Bottom)], help: "top / bottom", group: "move", pages: TODO },
    Binding { keys: &[(&[Key(Char('{')), Ctrl('u')], A::PrevSection), (&[Key(Char('}')), Ctrl('d')], A::NextSection)], help: "previous / next section", group: "move", pages: TODO },
    Binding { keys: &[(&[Key(Char('1'))], A::JumpSection(0)), (&[Key(Char('2'))], A::JumpSection(1)), (&[Key(Char('3'))], A::JumpSection(2)), (&[Key(Char('4'))], A::JumpSection(3)), (&[Key(Char('5'))], A::JumpSection(4)), (&[Key(Char('6'))], A::JumpSection(5)), (&[Key(Char('7'))], A::JumpSection(6)), (&[Key(Char('8'))], A::JumpSection(7)), (&[Key(Char('9'))], A::JumpSection(8))], help: "jump to section", group: "move", pages: TODO },
    Binding { keys: &[(&[Key(Char('J')), Shift(Down)], A::MoveDown), (&[Key(Char('K')), Shift(Up)], A::MoveUp)], help: "move down / up", group: "change", pages: TODO },
    Binding { keys: &[(&[Key(Enter)], A::Open)], help: "open in $EDITOR", group: "change", pages: TODO },
    Binding { keys: &[(&[Key(Char('o'))], A::NewBelow), (&[Key(Char('O'))], A::NewAbove)], help: "new task below / above", group: "change", pages: TODO },
    Binding { keys: &[(&[Key(Char('N'))], A::NewSection)], help: "new section", group: "change", pages: TODO },
    Binding { keys: &[(&[Key(Char('X'))], A::Done)], help: "done (archive)", group: "change", pages: TODO },
    Binding { keys: &[(&[Key(Char('D'))], A::Delete)], help: "delete", group: "change", pages: TODO },
    Binding { keys: &[(&[Key(Char('u'))], A::Undo)], help: "undo", group: "change", pages: TODO },
    Binding { keys: &[(&[Key(Char('h')), Key(Left)], A::Left), (&[Key(Char('l')), Key(Right)], A::Right)], help: "left / right", group: "heatmap", pages: ARCHIVE },
    Binding { keys: &[(&[Key(Char('j')), Key(Down)], A::Down), (&[Key(Char('k')), Key(Up)], A::Up)], help: "down / up", group: "heatmap", pages: ARCHIVE },
    Binding { keys: &[(&[Key(Char('H'))], A::SkipLeft), (&[Key(Char('L'))], A::SkipRight)], help: "left / right, skip empty", group: "heatmap", pages: ARCHIVE },
    Binding { keys: &[(&[Key(Char('J'))], A::SkipDown), (&[Key(Char('K'))], A::SkipUp)], help: "down / up, skip empty", group: "heatmap", pages: ARCHIVE },
    Binding { keys: &[(&[Key(Char('1'))], A::JumpSection(0)), (&[Key(Char('2'))], A::JumpSection(1)), (&[Key(Char('3'))], A::JumpSection(2)), (&[Key(Char('4'))], A::JumpSection(3)), (&[Key(Char('5'))], A::JumpSection(4)), (&[Key(Char('6'))], A::JumpSection(5)), (&[Key(Char('7'))], A::JumpSection(6)), (&[Key(Char('8'))], A::JumpSection(7)), (&[Key(Char('9'))], A::JumpSection(8))], help: "jump to section in the list", group: "heatmap", pages: ARCHIVE },
    Binding { keys: &[(&[Key(Char('j')), Key(Down)], A::Down), (&[Key(Char('k')), Key(Up)], A::Up)], help: "down / up", group: "list", pages: ARCHIVE },
    Binding { keys: &[(&[Seq('g', 'g'), Key(Home)], A::Top), (&[Key(Char('G')), Key(End)], A::Bottom)], help: "top / bottom", group: "list", pages: ARCHIVE },
    Binding { keys: &[(&[Key(Char('{')), Ctrl('u')], A::PrevSection), (&[Key(Char('}')), Ctrl('d')], A::NextSection)], help: "previous / next section", group: "list", pages: ARCHIVE },
    Binding { keys: &[(&[Key(Char('1'))], A::JumpSection(0)), (&[Key(Char('2'))], A::JumpSection(1)), (&[Key(Char('3'))], A::JumpSection(2)), (&[Key(Char('4'))], A::JumpSection(3)), (&[Key(Char('5'))], A::JumpSection(4)), (&[Key(Char('6'))], A::JumpSection(5)), (&[Key(Char('7'))], A::JumpSection(6)), (&[Key(Char('8'))], A::JumpSection(7)), (&[Key(Char('9'))], A::JumpSection(8))], help: "jump to section", group: "list", pages: ARCHIVE },
    Binding { keys: &[(&[Key(Char('p'))], A::Preview)], help: "toggle preview", group: "list", pages: ARCHIVE },
    Binding { keys: &[(&[Key(Char('p'))], A::Preview)], help: "toggle preview", group: "other", pages: TODO },
    Binding { keys: &[(&[Key(Char('/'))], A::Filter)], help: "filter", group: "other", pages: TODO },
    Binding { keys: &[(&[Key(Esc)], A::FilterClear)], help: "clear filter", group: "other", pages: TODO },
    Binding { keys: &[(&[Key(Tab)], A::Focus)], help: "keys to heatmap / list", group: "other", pages: ARCHIVE },
    Binding { keys: &[(&[Key(Char('?'))], A::Help)], help: "toggle help", group: "other", pages: ALL },
    Binding { keys: &[(&[Key(Char('['))], A::PrevPage), (&[Key(Char(']'))], A::NextPage)], help: "previous / next page", group: "other", pages: ALL },
    Binding { keys: &[(&[Key(Char('q')), Ctrl('c')], A::Quit)], help: "quit", group: "other", pages: ALL },
];

impl Binding {
    /// Display form of the triggers: `j` and `k`, `gg`, `q` and `C-c`. Arrows, `Home` / `End`,
    /// shifted arrows and `C-u` / `C-d` only clutter the help, the README lists them. The
    /// section digits are one `1-9`, nine labels would not fit.
    pub fn labels(&self) -> Vec<String> {
        self.triggers()
            .filter_map(|(trigger, action)| match (trigger, action) {
                (_, Action::JumpSection(0)) => {
                    let last = (0..).map_while(jump_key).last()?;
                    Some(format!("{}-{last}", jump_key(0)?))
                }
                (_, Action::JumpSection(_))
                | (Key(Up | Down | Left | Right | Home | End) | Shift(_) | Ctrl('u' | 'd'), _) => {
                    None
                }
                (Key(Char(c)), _) => Some(c.to_string()),
                (Key(Enter), _) => Some("<CR>".to_string()),
                (Key(other), _) => Some(format!("{other:?}")),
                (Ctrl(c), _) => Some(format!("C-{c}")),
                (Seq(a, b), _) => Some(format!("{a}{b}")),
            })
            .collect()
    }

    fn triggers(&self) -> impl Iterator<Item = (Trigger, Action)> {
        self.keys
            .iter()
            .flat_map(|(triggers, action)| triggers.iter().map(|t| (*t, *action)))
    }
}

/// The key that jumps to section `s`, `None` for a section without one.
pub fn jump_key(s: usize) -> Option<char> {
    BINDINGS
        .iter()
        .flat_map(Binding::triggers)
        .find_map(|(trigger, action)| match (trigger, action) {
            (Key(Char(c)), Action::JumpSection(n)) if n == s => Some(c),
            _ => None,
        })
}

/// Translates key presses into [`Action`]s, including multi-key sequences like `gg`.
#[derive(Debug, Default)]
pub struct Keymap {
    /// First key of an unfinished sequence, e.g. `g` after the first `g` of `gg`.
    pending: Option<char>,
}

impl Keymap {
    /// Returns `None` while a sequence is incomplete and for keys not mapped on `page`. `Esc`
    /// during a sequence only cancels it.
    pub fn handle(&mut self, key: KeyEvent, page: Page) -> Option<Action> {
        let triggers = BINDINGS
            .iter()
            .filter(|binding| binding.pages.contains(&page))
            .flat_map(Binding::triggers);
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
        None
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
        return match key.code {
            Char('c') => Some(Action::Quit),
            Char('w') => Some(Action::FilterDeleteWord),
            _ => None,
        };
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
    use super::*;

    fn press(keymap: &mut Keymap, code: KeyCode) -> Option<Action> {
        keymap.handle(KeyEvent::from(code), Page::Todo)
    }

    #[test]
    fn quit_keys() {
        let mut keymap = Keymap::default();
        assert_eq!(press(&mut keymap, KeyCode::Char('q')), Some(Action::Quit));
        let ctrl_c = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
        assert_eq!(keymap.handle(ctrl_c, Page::Todo), Some(Action::Quit));
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
        for binding in BINDINGS {
            for (&page, (trigger, expected)) in binding
                .pages
                .iter()
                .flat_map(|page| binding.triggers().map(move |t| (page, t)))
            {
                let mut keymap = Keymap::default();
                let mut press = |event| keymap.handle(event, page);
                let action = match trigger {
                    Key(code) => press(KeyEvent::from(code)),
                    Ctrl(c) => press(KeyEvent::new(Char(c), KeyModifiers::CONTROL)),
                    Shift(code) => press(KeyEvent::new(code, KeyModifiers::SHIFT)),
                    Seq(a, b) => {
                        press(KeyEvent::from(Char(a)));
                        press(KeyEvent::from(Char(b)))
                    }
                };
                assert_eq!(action, Some(expected), "trigger {trigger:?} on {page:?}");
            }
        }
    }

    #[test]
    fn labels() {
        let labels = |help| BINDINGS.iter().find(|b| b.help == help).unwrap().labels();
        assert_eq!(labels("down / up"), ["j", "k"]);
        assert_eq!(labels("delete"), ["D"]);
        assert_eq!(labels("top / bottom"), ["gg", "G"]);
        assert_eq!(labels("previous / next section"), ["{", "}"]);
        assert_eq!(labels("move down / up"), ["J", "K"]);
        assert_eq!(labels("quit"), ["q", "C-c"]);
        assert_eq!(labels("jump to section"), ["1-9"]);
    }

    #[test]
    fn shift_arrow_moves_instead_of_cursor() {
        let mut keymap = Keymap::default();
        let shift_down = KeyEvent::new(Down, KeyModifiers::SHIFT);
        assert_eq!(
            keymap.handle(shift_down, Page::Todo),
            Some(Action::MoveDown)
        );
        // Terminals send `J` with the shift modifier set, it still is `J`.
        let shift_j = KeyEvent::new(Char('J'), KeyModifiers::SHIFT);
        assert_eq!(keymap.handle(shift_j, Page::Todo), Some(Action::MoveDown));
        assert_eq!(press(&mut keymap, Down), Some(Action::Down));
    }

    #[test]
    fn ctrl_does_not_trigger_plain_keys() {
        let mut keymap = Keymap::default();
        let ctrl_j = KeyEvent::new(Char('j'), KeyModifiers::CONTROL);
        assert_eq!(keymap.handle(ctrl_j, Page::Todo), None);
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
        assert_eq!(keymap.handle(ctrl_c, Page::Todo), Some(Action::Quit));
        assert_eq!(keymap.pending(), None);
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
        assert_eq!(ctrl('w'), Some(Action::FilterDeleteWord));
        assert_eq!(ctrl('j'), None);
    }
}
