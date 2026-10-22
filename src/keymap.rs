//! Keys to [`Action`]s. All bindings live in [`BINDINGS`], the help overlay and the footer read
//! it too.

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
    /// `v` on the archive page: starts a range at the selected day, or ends it.
    Visual,
    /// `<CR>` on the archive page: the list takes the move keys, it shows the selected day.
    FocusList,
    /// `Esc` on the archive page: the heatmap takes the move keys back.
    FocusHeatmap,
    /// `p`: shows or hides the preview of the selected task, on each page of its own.
    Preview,
    /// `.`: shows or hides the repeating tasks that are not due, like hidden files.
    ShowHidden,
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
    /// `S-Tab` / `Tab`: the page before or after the current one, see [`Page`].
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
    /// `Esc` in the filter input: closes it and clears the filter.
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

/// Where keys go: a page, or what is open over it and takes all keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Context {
    Page(Page),
    /// The help overlay, it lists the bindings of the page below it.
    Help,
    /// The filter input: keys without a binding here type into the query.
    Filter,
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
    pub contexts: &'static [Context],
}

/// Bindings of the task list only.
const TASKS: &[Context] = &[Context::Page(Page::Tasks)];

/// Bindings of the archive page only.
const ARCHIVE: &[Context] = &[Context::Page(Page::Archive)];

/// Bindings that work on every page.
const PAGES: &[Context] = &[Context::Page(Page::Tasks), Context::Page(Page::Archive)];

/// Bindings of the help overlay.
const HELP: &[Context] = &[Context::Help];

/// Bindings of the filter input.
const FILTER: &[Context] = &[Context::Filter];

use Action as A;
use KeyCode::{BackTab, Backspace, Char, Down, End, Enter, Esc, Home, Left, Right, Tab, Up};
use Trigger::{Ctrl, Key, Seq, Shift};

/// The single source of truth for keys: [`Keymap::handle`] reads it and so does the help
/// overlay, so the two can never disagree. A key may be in several groups of a page, like `j` in
/// the archive's heatmap and list, it does the same there. One line per binding, hence no
/// rustfmt.
#[rustfmt::skip]
pub const BINDINGS: &[Binding] = &[
    Binding { keys: &[(&[Key(Char('j')), Key(Down)], A::Down), (&[Key(Char('k')), Key(Up)], A::Up)], help: "down / up", group: "move", contexts: TASKS },
    Binding { keys: &[(&[Seq('g', 'g'), Key(Home)], A::Top), (&[Key(Char('G')), Key(End)], A::Bottom)], help: "top / bottom", group: "move", contexts: TASKS },
    Binding { keys: &[(&[Key(Char('{'))], A::PrevSection), (&[Key(Char('}'))], A::NextSection)], help: "previous / next section", group: "move", contexts: TASKS },
    Binding { keys: &[(&[Key(Char('1'))], A::JumpSection(0)), (&[Key(Char('2'))], A::JumpSection(1)), (&[Key(Char('3'))], A::JumpSection(2)), (&[Key(Char('4'))], A::JumpSection(3)), (&[Key(Char('5'))], A::JumpSection(4)), (&[Key(Char('6'))], A::JumpSection(5)), (&[Key(Char('7'))], A::JumpSection(6)), (&[Key(Char('8'))], A::JumpSection(7)), (&[Key(Char('9'))], A::JumpSection(8))], help: "jump to section by its number", group: "move", contexts: TASKS },
    Binding { keys: &[(&[Key(Char('J')), Shift(Down)], A::MoveDown), (&[Key(Char('K')), Shift(Up)], A::MoveUp)], help: "move down / up", group: "change", contexts: TASKS },
    Binding { keys: &[(&[Key(Enter)], A::Open)], help: "open task in $EDITOR, or rename section", group: "change", contexts: TASKS },
    Binding { keys: &[(&[Key(Char('o'))], A::NewBelow), (&[Key(Char('O'))], A::NewAbove)], help: "new task below / above", group: "change", contexts: TASKS },
    Binding { keys: &[(&[Key(Char('N'))], A::NewSection)], help: "new section", group: "change", contexts: TASKS },
    Binding { keys: &[(&[Key(Char('X'))], A::Done)], help: "done: archive, a repeating task hides", group: "change", contexts: TASKS },
    Binding { keys: &[(&[Key(Char('D'))], A::Delete)], help: "delete task, or empty section", group: "change", contexts: TASKS },
    Binding { keys: &[(&[Key(Char('u'))], A::Undo)], help: "undo the last change", group: "change", contexts: TASKS },
    Binding { keys: &[(&[Key(Char('h')), Key(Left)], A::Left), (&[Key(Char('l')), Key(Right)], A::Right)], help: "previous / next week", group: "heatmap", contexts: ARCHIVE },
    Binding { keys: &[(&[Key(Char('j')), Key(Down)], A::Down), (&[Key(Char('k')), Key(Up)], A::Up)], help: "next / previous day", group: "heatmap", contexts: ARCHIVE },
    Binding { keys: &[(&[Seq('g', 'g'), Key(Home)], A::Top), (&[Key(Char('G')), Key(End)], A::Bottom)], help: "first day done / today", group: "heatmap", contexts: ARCHIVE },
    Binding { keys: &[(&[Key(Char('v'))], A::Visual)], help: "select days from here, v / Esc ends", group: "heatmap", contexts: ARCHIVE },
    Binding { keys: &[(&[Key(Char('H'))], A::SkipLeft), (&[Key(Char('L'))], A::SkipRight)], help: "like h / l, skips empty days", group: "heatmap", contexts: ARCHIVE },
    Binding { keys: &[(&[Key(Char('J'))], A::SkipDown), (&[Key(Char('K'))], A::SkipUp)], help: "like j / k, skips empty days", group: "heatmap", contexts: ARCHIVE },
    Binding { keys: &[(&[Key(Char('1'))], A::JumpSection(0)), (&[Key(Char('2'))], A::JumpSection(1)), (&[Key(Char('3'))], A::JumpSection(2)), (&[Key(Char('4'))], A::JumpSection(3)), (&[Key(Char('5'))], A::JumpSection(4)), (&[Key(Char('6'))], A::JumpSection(5)), (&[Key(Char('7'))], A::JumpSection(6)), (&[Key(Char('8'))], A::JumpSection(7)), (&[Key(Char('9'))], A::JumpSection(8))], help: "jump to a section in the list", group: "heatmap", contexts: ARCHIVE },
    Binding { keys: &[(&[Key(Enter)], A::FocusList)], help: "go to the tasks of the selected days", group: "heatmap", contexts: ARCHIVE },
    Binding { keys: &[(&[Key(Char('j')), Key(Down)], A::Down), (&[Key(Char('k')), Key(Up)], A::Up)], help: "down / up", group: "list", contexts: ARCHIVE },
    Binding { keys: &[(&[Seq('g', 'g'), Key(Home)], A::Top), (&[Key(Char('G')), Key(End)], A::Bottom)], help: "top / bottom", group: "list", contexts: ARCHIVE },
    Binding { keys: &[(&[Key(Char('{'))], A::PrevSection), (&[Key(Char('}'))], A::NextSection)], help: "previous / next section", group: "list", contexts: ARCHIVE },
    Binding { keys: &[(&[Key(Char('1'))], A::JumpSection(0)), (&[Key(Char('2'))], A::JumpSection(1)), (&[Key(Char('3'))], A::JumpSection(2)), (&[Key(Char('4'))], A::JumpSection(3)), (&[Key(Char('5'))], A::JumpSection(4)), (&[Key(Char('6'))], A::JumpSection(5)), (&[Key(Char('7'))], A::JumpSection(6)), (&[Key(Char('8'))], A::JumpSection(7)), (&[Key(Char('9'))], A::JumpSection(8))], help: "jump to section by its number", group: "list", contexts: ARCHIVE },
    Binding { keys: &[(&[Key(Char('p'))], A::Preview)], help: "toggle preview of the task", group: "list", contexts: ARCHIVE },
    Binding { keys: &[(&[Key(Esc)], A::FocusHeatmap)], help: "back to the heatmap", group: "list", contexts: ARCHIVE },
    Binding { keys: &[(&[Key(Char('p'))], A::Preview)], help: "toggle preview of the task", group: "show", contexts: TASKS },
    Binding { keys: &[(&[Key(Char('.'))], A::ShowHidden)], help: "toggle hidden tasks: repeating, not due", group: "show", contexts: TASKS },
    Binding { keys: &[(&[Key(Char('/'))], A::Filter)], help: "filter titles and bodies", group: "global", contexts: PAGES },
    Binding { keys: &[(&[Key(Char('?'))], A::Help)], help: "toggle help", group: "global", contexts: PAGES },
    Binding { keys: &[(&[Key(Tab)], A::NextPage), (&[Key(BackTab)], A::PrevPage)], help: "next / previous page", group: "global", contexts: PAGES },
    Binding { keys: &[(&[Key(Char('q')), Ctrl('c')], A::Quit)], help: "quit", group: "global", contexts: &[Context::Page(Page::Tasks), Context::Page(Page::Archive), Context::Help] },
    // Other keys do nothing in the help, so a stray key cannot change tasks dimmed behind it.
    Binding { keys: &[(&[Key(Char('j')), Key(Down)], A::Down), (&[Key(Char('k')), Key(Up)], A::Up)], help: "scroll down / up", group: "help", contexts: HELP },
    Binding { keys: &[(&[Seq('g', 'g'), Key(Home)], A::Top), (&[Key(Char('G')), Key(End)], A::Bottom)], help: "top / bottom", group: "help", contexts: HELP },
    Binding { keys: &[(&[Key(Char('/'))], A::Filter)], help: "filter keys", group: "help", contexts: HELP },
    Binding { keys: &[(&[Key(Char('?')), Key(Esc)], A::Help)], help: "close help", group: "help", contexts: HELP },
    Binding { keys: &[(&[Key(Enter)], A::FilterAccept)], help: "keep filter", group: "filter", contexts: FILTER },
    Binding { keys: &[(&[Key(Esc)], A::FilterClear)], help: "clear filter", group: "filter", contexts: FILTER },
    Binding { keys: &[(&[Key(Backspace)], A::FilterBackspace)], help: "delete a character", group: "filter", contexts: FILTER },
    Binding { keys: &[(&[Ctrl('w')], A::FilterDeleteWord)], help: "delete a word", group: "filter", contexts: FILTER },
    Binding { keys: &[(&[Ctrl('c')], A::Quit)], help: "quit", group: "filter", contexts: FILTER },
];

impl Binding {
    /// The first key of each action: `j` and `k`, `gg`, `q`. The section digits are one `1-9`,
    /// nine labels would not fit.
    pub fn labels(&self) -> Vec<String> {
        let first = |(triggers, _): &(&[Trigger], Action)| label(triggers[0]);
        match self.keys {
            [first_key @ (_, Action::JumpSection(_)), .., last_key] => {
                vec![format!("{}-{}", first(first_key), first(last_key))]
            }
            keys => keys.iter().map(first).collect(),
        }
    }

    /// The other keys of the actions: `↓` and `↑` for people outside vim, `C-c`.
    pub fn second_labels(&self) -> Vec<String> {
        (self.keys.iter())
            .flat_map(|(triggers, _)| triggers[1..].iter().map(|&trigger| label(trigger)))
            .collect()
    }

    fn triggers(&self) -> impl Iterator<Item = (Trigger, Action)> {
        self.keys
            .iter()
            .flat_map(|(triggers, action)| triggers.iter().map(|t| (*t, *action)))
    }
}

/// Display form of a trigger, in vim's notation where it has one: `<CR>`, `C-c`.
fn label(trigger: Trigger) -> String {
    let code_label = |code| match code {
        Up => "↑".to_string(),
        Down => "↓".to_string(),
        Left => "←".to_string(),
        Right => "→".to_string(),
        Enter => "<CR>".to_string(),
        Backspace => "<BS>".to_string(),
        BackTab => "S-Tab".to_string(),
        Char(c) => c.to_string(),
        other => format!("{other:?}"),
    };
    match trigger {
        Key(code) => code_label(code),
        Shift(code) => format!("S-{}", code_label(code)),
        Ctrl(c) => format!("C-{c}"),
        Seq(a, b) => format!("{a}{b}"),
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
    /// Returns `None` while a sequence is incomplete and for keys not mapped in `context`. `Esc`
    /// during a sequence only cancels it.
    pub fn handle(&mut self, key: KeyEvent, context: Context) -> Option<Action> {
        let mut triggers = BINDINGS
            .iter()
            .filter(|binding| binding.contexts.contains(&context))
            .flat_map(Binding::triggers);
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);

        if ctrl {
            // Ctrl keys work even halfway through a sequence, so `C-c` always quits.
            self.pending = None;
            let KeyCode::Char(c) = key.code else {
                return None;
            };
            return triggers.find_map(|(t, action)| (t == Ctrl(c)).then_some(action));
        }

        // Any key ends a pending sequence, whether it completes it or not (like vim).
        if let Some(first) = self.pending.take() {
            let KeyCode::Char(second) = key.code else {
                return None;
            };
            return triggers.find_map(|(t, action)| (t == Seq(first, second)).then_some(action));
        }

        // Shift only counts for non-letters: a shifted letter already arrives as `J`, a shifted
        // `Tab` as `BackTab`.
        let shift =
            key.modifiers.contains(KeyModifiers::SHIFT) && !matches!(key.code, Char(_) | BackTab);
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
        match key.code {
            Char(c) if context == Context::Filter => Some(Action::FilterType(c)),
            _ => None,
        }
    }

    /// The unfinished sequence, to show in the status.
    pub fn pending(&self) -> Option<char> {
        self.pending
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn press(keymap: &mut Keymap, code: KeyCode) -> Option<Action> {
        keymap.handle(KeyEvent::from(code), Context::Page(Page::Tasks))
    }

    #[test]
    fn quit_keys() {
        let mut keymap = Keymap::default();
        assert_eq!(press(&mut keymap, KeyCode::Char('q')), Some(Action::Quit));
        let ctrl_c = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
        assert_eq!(
            keymap.handle(ctrl_c, Context::Page(Page::Tasks)),
            Some(Action::Quit)
        );
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
            for (&context, (trigger, expected)) in binding
                .contexts
                .iter()
                .flat_map(|context| binding.triggers().map(move |t| (context, t)))
            {
                let mut keymap = Keymap::default();
                let mut press = |event| keymap.handle(event, context);
                let action = match trigger {
                    Key(code) => press(KeyEvent::from(code)),
                    Ctrl(c) => press(KeyEvent::new(Char(c), KeyModifiers::CONTROL)),
                    Shift(code) => press(KeyEvent::new(code, KeyModifiers::SHIFT)),
                    Seq(a, b) => {
                        press(KeyEvent::from(Char(a)));
                        press(KeyEvent::from(Char(b)))
                    }
                };
                assert_eq!(action, Some(expected), "trigger {trigger:?} in {context:?}");
            }
        }
    }

    #[test]
    fn labels() {
        let binding = |help| BINDINGS.iter().find(|b| b.help == help).unwrap();
        let labels = |help| binding(help).labels();
        assert_eq!(labels("down / up"), ["j", "k"]);
        assert_eq!(labels("delete task, or empty section"), ["D"]);
        assert_eq!(labels("top / bottom"), ["gg", "G"]);
        assert_eq!(labels("previous / next section"), ["{", "}"]);
        assert_eq!(labels("quit"), ["q"]);
        assert_eq!(labels("jump to section by its number"), ["1-9"]);
        assert_eq!(labels("next / previous page"), ["Tab", "S-Tab"]);
        assert_eq!(labels("open task in $EDITOR, or rename section"), ["<CR>"]);

        let second = |help| binding(help).second_labels();
        assert_eq!(second("down / up"), ["↓", "↑"]);
        assert_eq!(second("top / bottom"), ["Home", "End"]);
        assert_eq!(second("move down / up"), ["S-↓", "S-↑"]);
        assert_eq!(second("quit"), ["C-c"]);
        assert_eq!(
            second("jump to section by its number"),
            Vec::<String>::new()
        );
    }

    #[test]
    fn shift_arrow_moves_instead_of_cursor() {
        let mut keymap = Keymap::default();
        let shift_down = KeyEvent::new(Down, KeyModifiers::SHIFT);
        assert_eq!(
            keymap.handle(shift_down, Context::Page(Page::Tasks)),
            Some(Action::MoveDown)
        );
        // Terminals send `J` with the shift modifier set, it still is `J`.
        let shift_j = KeyEvent::new(Char('J'), KeyModifiers::SHIFT);
        assert_eq!(
            keymap.handle(shift_j, Context::Page(Page::Tasks)),
            Some(Action::MoveDown)
        );
        assert_eq!(press(&mut keymap, Down), Some(Action::Down));
    }

    #[test]
    fn ctrl_does_not_trigger_plain_keys() {
        let mut keymap = Keymap::default();
        let ctrl_j = KeyEvent::new(Char('j'), KeyModifiers::CONTROL);
        assert_eq!(keymap.handle(ctrl_j, Context::Page(Page::Tasks)), None);
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
        assert_eq!(
            keymap.handle(ctrl_c, Context::Page(Page::Tasks)),
            Some(Action::Quit)
        );
        assert_eq!(keymap.pending(), None);
    }

    #[test]
    fn unmapped_key() {
        let mut keymap = Keymap::default();
        assert_eq!(press(&mut keymap, KeyCode::Char('z')), None);
    }

    #[test]
    fn esc_only_backs_out() {
        let mut keymap = Keymap::default();
        // The filter is cleared from its input, `Esc` on the task list does nothing.
        assert_eq!(press(&mut keymap, KeyCode::Esc), None);
        let esc = KeyEvent::from(KeyCode::Esc);
        assert_eq!(
            keymap.handle(esc, Context::Page(Page::Archive)),
            Some(Action::FocusHeatmap)
        );
    }

    #[test]
    fn shift_tab_is_the_previous_page() {
        let mut keymap = Keymap::default();
        // Terminals send `BackTab`, some with the shift modifier set.
        let back_tab = KeyEvent::new(BackTab, KeyModifiers::SHIFT);
        assert_eq!(
            keymap.handle(back_tab, Context::Page(Page::Tasks)),
            Some(Action::PrevPage)
        );
        assert_eq!(press(&mut keymap, BackTab), Some(Action::PrevPage));
        assert_eq!(press(&mut keymap, Tab), Some(Action::NextPage));
    }

    #[test]
    fn filter_input_takes_text() {
        let mut keymap = Keymap::default();
        let mut key = |code| keymap.handle(KeyEvent::from(code), Context::Filter);
        // Keys that are bindings elsewhere are text here.
        assert_eq!(key(Char('j')), Some(Action::FilterType('j')));
        assert_eq!(key(Char('?')), Some(Action::FilterType('?')));
        assert_eq!(key(Backspace), Some(Action::FilterBackspace));
        assert_eq!(key(Enter), Some(Action::FilterAccept));
        assert_eq!(key(Esc), Some(Action::FilterClear));
        assert_eq!(key(Down), None);
        let mut keymap = Keymap::default();
        let mut ctrl = |c| {
            keymap.handle(
                KeyEvent::new(Char(c), KeyModifiers::CONTROL),
                Context::Filter,
            )
        };
        assert_eq!(ctrl('c'), Some(Action::Quit));
        assert_eq!(ctrl('w'), Some(Action::FilterDeleteWord));
        assert_eq!(ctrl('j'), None);
    }
}
