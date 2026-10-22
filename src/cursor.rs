//! Cursor moves over rows with section headers: the task list and the done tasks on the archive
//! page alike. Pure, the lists map their rows to [`Line`]s.

use crate::keymap::Action;

/// What a row is to the cursor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Line {
    /// The header of the section with this index.
    Section(usize),
    Task,
}

/// Where `action` moves the cursor at `cursor`. Stays for actions that are no move, and where
/// there is nothing to move to.
pub fn moved(lines: &[Line], cursor: usize, action: Action) -> usize {
    let last = lines.len().saturating_sub(1);
    let header = |i: &usize| matches!(lines[*i], Line::Section(_));
    match action {
        Action::Up => cursor.saturating_sub(1),
        Action::Down => (cursor + 1).min(last),
        Action::Top => 0,
        Action::Bottom => last,
        // On a row that is its own section.
        Action::PrevSection => (0..cursor).rev().find(header).unwrap_or(0),
        Action::NextSection => (cursor + 1..lines.len()).find(header).unwrap_or(cursor),
        // The first task of the section, its header if it has none. Stays if it is not there.
        Action::JumpSection(s) => match lines.iter().position(|&l| l == Line::Section(s)) {
            Some(at) => at + usize::from(lines.get(at + 1) == Some(&Line::Task)),
            None => cursor,
        },
        _ => cursor,
    }
}
