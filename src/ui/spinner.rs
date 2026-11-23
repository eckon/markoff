//! A spinner for work that runs on a thread, so a wait shows that something is happening.
//! Only after [`DELAY`], a quick wait would only flash it.

use std::time::Duration;

use ratatui::{style::Stylize, text::Line};

use super::CURRENT_COLOR;

/// How long a wait goes without a spinner.
const DELAY: Duration = Duration::from_millis(200);
/// How long each frame shows.
const FRAME: Duration = Duration::from_millis(80);
const FRAMES: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

/// The frame after waiting for `waited`, `None` while it is shorter than [`DELAY`].
pub fn frame(waited: Duration) -> Option<usize> {
    (waited >= DELAY).then(|| (waited.as_millis() / FRAME.as_millis()) as usize)
}

/// `⠋ text`, the spinner at `frame` and what is waited for.
pub fn line(frame: usize, text: &str) -> Line<'static> {
    Line::from(vec![
        FRAMES[frame % FRAMES.len()].fg(CURRENT_COLOR),
        format!(" {text}").dim(),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shows_after_the_delay_and_turns() {
        assert_eq!(frame(Duration::ZERO), None);
        assert_eq!(frame(DELAY - Duration::from_millis(1)), None);
        let first = frame(DELAY).unwrap();
        assert_eq!(frame(DELAY + FRAME), Some(first + 1));
        assert_eq!(line(0, "loading").to_string(), "⠋ loading");
        assert_eq!(line(FRAMES.len() + 1, "loading").to_string(), "⠙ loading");
    }
}
