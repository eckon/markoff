//! Cursor moves over rows with section headers: the todo list and the done tasks on the archive
//! page alike. Pure, the lists map their rows to [`Line`]s.

use crate::keymap::Action;

/// What a row is to the cursor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Line {
    /// The header of the section with this index, `None` for one without a key (daily).
    Header(Option<usize>),
    Item,
}

/// Where `action` moves the cursor at `cursor`. Stays for actions that are no move, and where
/// there is nothing to move to.
pub fn moved(lines: &[Line], cursor: usize, action: Action) -> usize {
    let last = lines.len().saturating_sub(1);
    let header = |i: &usize| matches!(lines[*i], Line::Header(_));
    match action {
        Action::Up => cursor.saturating_sub(1),
        Action::Down => (cursor + 1).min(last),
        Action::Top => 0,
        Action::Bottom => last,
        // On an item that is its own section.
        Action::PrevSection => (0..cursor).rev().find(header).unwrap_or(0),
        Action::NextSection => (cursor + 1..lines.len()).find(header).unwrap_or(cursor),
        // The first item of the section, its header if it has none. Stays if it is not there.
        Action::JumpSection(s) => match lines.iter().position(|&l| l == Line::Header(Some(s))) {
            Some(at) => at + usize::from(lines.get(at + 1) == Some(&Line::Item)),
            None => cursor,
        },
        _ => cursor,
    }
}
