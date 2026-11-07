//! The tasks page: the task list with the preview of the selected task next to it.

use ratatui::{
    Frame,
    layout::Rect,
    style::{Style, Stylize},
};

use super::{filtered_title, list_with_preview, rows, section_color};
use crate::app::{App, Row};

/// The list, and the preview of the selected task next to it unless `p` hid it. Returns
/// the list's offset and the column of the preview's divider.
pub fn render(app: &App, frame: &mut Frame, main_area: Rect) -> (usize, Option<u16>) {
    let body = match app.selected() {
        _ if app.hide_preview => None,
        Some(Row::Task(s, t)) => Some(app.sections[s].tasks[t].body.as_str()),
        Some(Row::Section(_)) => Some(""),
        // Nothing to preview without rows, the note about it gets the width.
        None => None,
    };
    let (offset, preview) =
        list_with_preview(frame, main_area, body, &app.filter, |frame, area, width| {
            render_list(app, frame, area, width)
        });
    (offset, preview.map(|preview| preview.x))
}

/// The task list: the sections with their tasks, see [`rows`].
/// Returns the scroll offset.
fn render_list(app: &App, frame: &mut Frame, area: Rect, width: usize) -> usize {
    let rows = app.rows();
    let items: Vec<rows::Row> = (rows.iter().enumerate())
        .map(|(i, &row)| {
            let (Row::Section(s) | Row::Task(s, _)) = row;
            let color = section_color(s);
            let kind = match row {
                Row::Section(s) => {
                    // Only the shown tasks, so with a filter it says how many match.
                    let count = rows
                        .iter()
                        .filter(|row| matches!(row, Row::Task(task_s, _) if *task_s == s))
                        .count();
                    let name = &app.sections[s].name;
                    rows::Kind::Section(rows::section_header(
                        s,
                        color,
                        name,
                        count,
                        width.saturating_sub(1),
                    ))
                }
                Row::Task(s, t) => {
                    let task = &app.sections[s].tasks[t];
                    let title = task.title();
                    let mut style = match i == app.cursor {
                        true => Style::new().fg(color),
                        false => Style::new(),
                    };
                    // Shown by `.` only: weaker and struck through, it is not due.
                    let due = task.due(app.today);
                    if !due {
                        style = style.dim().crossed_out();
                    }
                    // Struck through by hand, the heading no longer shows the `~~`.
                    if task.canceled {
                        style = style.crossed_out();
                    }
                    let spans = filtered_title(&title, &app.filter, style);
                    // Why it is there, where the archive puts the day. Struck through like the
                    // title while it is not due.
                    let rule = task.repeat.as_ref().map(|repeat| {
                        let rule = repeat.describe().dim();
                        if due { rule } else { rule.crossed_out() }
                    });
                    rows::Kind::Task(spans, rule, task.repeat.is_some())
                }
            };
            rows::Row { color, kind }
        })
        .collect();

    let mut notes = Vec::new();
    if items.is_empty() && !app.filter.is_empty() {
        notes.push("no matches");
    }
    // Tasks only live in sections, so without one there is nowhere to add a task.
    if app.sections.is_empty() {
        notes.push("no sections yet, N creates one");
    }
    rows::render(
        frame,
        area,
        items,
        (Some(app.cursor), app.list_offset),
        &notes,
    )
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use ratatui::{
        Terminal,
        backend::TestBackend,
        style::{Color, Modifier},
    };

    use super::*;
    use crate::{
        app::tests::{TODAY, sections},
        keymap::Action,
        ui::{render, tests::screen},
    };

    /// Draws like the main loop: the scroll offset of one frame is where the next starts.
    fn draw(app: &mut App) -> Vec<String> {
        let mut offset = app.list_offset;
        let mut terminal = Terminal::new(TestBackend::new(50, 9)).unwrap();
        terminal
            .draw(|frame| offset = render(app, frame).list_offset)
            .unwrap();
        app.list_offset = offset;
        screen(app).lines().map(str::to_string).collect()
    }

    #[test]
    fn list_scrolls_only_when_the_cursor_leaves_the_screen() {
        let titles: Vec<String> = (1..=20).map(|i| format!("Task {i}")).collect();
        let titles: Vec<&str> = titles.iter().map(String::as_str).collect();
        let mut app = App::new(
            PathBuf::new(),
            sections(&[("home", &titles)]),
            crate::config::defaults(),
        );

        app.update(Action::Bottom);
        let lines = draw(&mut app);
        assert!(lines[6].contains("▌   • Task 20"), "{lines:#?}");

        // Moving up inside the screen moves the cursor, the list does not scroll.
        app.update(Action::Up);
        draw(&mut app);
        app.update(Action::Up);
        let lines = draw(&mut app);
        assert!(lines[6].contains("• Task 20"), "{lines:#?}");
        assert!(lines[4].contains("▌   • Task 18"), "{lines:#?}");
    }

    #[test]
    fn long_titles_wrap_with_and_without_the_cursor() {
        let long = "Write the long report about every open question";
        let mut app = App::new(
            PathBuf::new(),
            sections(&[("home", &[long, long])]),
            crate::config::defaults(),
        );
        app.hide_preview = true;

        app.update(Action::Down);
        let lines = draw(&mut app);
        // 50 wide: padding, gutter and `   • ` leave 41 for the title.
        assert_eq!(
            lines[3],
            " ▌   • Write the long report about every open     "
        );
        assert_eq!(lines[4].trim_end(), " ▌     question");
        assert_eq!(
            lines[5].trim_end(),
            "     • Write the long report about every open"
        );
        assert_eq!(lines[6].trim_end(), "       question");
    }

    #[test]
    fn renders_sections_and_tasks() {
        let app = App::new(
            PathBuf::new(),
            sections(&[("home", &["Fix typo"]), ("work", &["Review PR"])]),
            crate::config::defaults(),
        );
        let screen = screen(&app);
        for text in ["──[1]─ home", "• Fix typo", "──[2]─ work", "• Review PR"] {
            assert!(screen.contains(text), "missing {text:?} in\n{screen}");
        }
    }

    #[test]
    fn sections_past_nine_have_no_key() {
        let names: Vec<String> = (1..=10).map(|i| format!("s{i}")).collect();
        let spec: Vec<(&str, &[&str])> = names.iter().map(|n| (n.as_str(), &[][..])).collect();
        let mut app = App::new(PathBuf::new(), sections(&spec), crate::config::defaults());
        app.update(Action::Bottom);
        let screen = screen(&app);
        // Same length as `──[9]─ `, so the names stay aligned.
        assert!(screen.contains("──[9]─ s9 "), "{screen}");
        assert!(screen.contains("────── s10 "), "{screen}");
    }

    #[test]
    fn task_struck_through_by_hand_stays_struck_through() {
        let mut app = App::new(
            PathBuf::new(),
            sections(&[("home", &["~~Fix typo~~"])]),
            crate::config::defaults(),
        );
        app.cursor = 1;
        app.hide_preview = true;
        let mut terminal = Terminal::new(TestBackend::new(40, 6)).unwrap();
        terminal.draw(|frame| _ = render(&app, frame)).unwrap();
        let buffer = terminal.backend().buffer();
        let line: String = (0..40).map(|x| buffer[(x, 3)].symbol()).collect();
        assert!(line.starts_with(" ▌   • Fix typo "), "{line:?}");
        assert!(buffer[(7, 3)].modifier.contains(Modifier::CROSSED_OUT));
    }

    #[test]
    fn repeating_task_shows_its_rule_while_due() {
        let mut app = App::new(
            PathBuf::new(),
            sections(&[("home", &["Water `* * mon-fri`", "Dust `* * *`", "Fix typo"])]),
            crate::config::defaults(),
        );
        app.today = TODAY;
        app.sections[0].tasks[1].last_done = Some(TODAY);
        app.hide_preview = true;
        let mut terminal = Terminal::new(TestBackend::new(60, 9)).unwrap();
        terminal.draw(|frame| _ = render(&app, frame)).unwrap();
        let lines: Vec<String> = (terminal.backend().buffer().content())
            .chunks(60)
            .map(|row| row.iter().map(|cell| cell.symbol()).collect())
            .collect();

        // Never done, so due: `⟳` and the rule dimmed on the right, before the padding. Done
        // today: hidden, also from the counts.
        assert!(lines[3].starts_with("     ⟳ Water "), "{lines:#?}");
        assert!(lines[3].ends_with(" weekdays  "), "{lines:#?}");
        assert!(
            terminal.backend().buffer()[(50, 3)]
                .modifier
                .contains(Modifier::DIM)
        );
        assert!(lines[4].contains("• Fix typo"), "{lines:#?}");
        assert!(lines[2].trim_end().ends_with(" 2"), "{lines:#?}");
        assert!(lines[0].ends_with(" 2 open"), "{lines:#?}");

        // `.` shows it dimmed, with its rule. Not open, the section counts what it shows.
        app.show_hidden = true;
        terminal.draw(|frame| _ = render(&app, frame)).unwrap();
        let buffer = terminal.backend().buffer();
        let lines: Vec<String> = (buffer.content().chunks(60))
            .map(|row| row.iter().map(|cell| cell.symbol()).collect())
            .collect();
        assert!(lines[4].starts_with("     ⟳ Dust "), "{lines:#?}");
        assert!(lines[4].ends_with(" daily  "), "{lines:#?}");
        // Title and rule struck through.
        assert!(buffer[(7, 4)].modifier.contains(Modifier::CROSSED_OUT));
        assert!(buffer[(53, 4)].modifier.contains(Modifier::CROSSED_OUT));
        assert!(buffer[(7, 4)].modifier.contains(Modifier::DIM));
        assert!(!buffer[(7, 3)].modifier.contains(Modifier::DIM), "due");
        assert!(lines[2].trim_end().ends_with(" 3"), "{lines:#?}");
        assert!(lines[0].ends_with(" 2 open"), "{lines:#?}");
    }

    #[test]
    fn filter_without_matches_says_so() {
        let mut app = App::new(
            PathBuf::new(),
            sections(&[("home", &["Fix typo"])]),
            crate::config::defaults(),
        );
        assert!(!screen(&app).contains("no matches"), "not without a filter");

        app.update(Action::Filter);
        app.update(Action::FilterType('x'));
        app.update(Action::FilterType('q'));
        assert!(screen(&app).lines().nth(2).unwrap().contains("no matches"));
    }

    #[test]
    fn hint_without_sections() {
        let mut app = App::new(PathBuf::new(), sections(&[]), crate::config::defaults());
        assert!(screen(&app).contains("no sections yet, N creates one"));

        app.sections = sections(&[("work", &[])]);
        assert!(!screen(&app).contains("no sections yet"));
    }

    #[test]
    fn list_highlights_filter_matches() {
        let mut app = App::new(
            PathBuf::new(),
            sections(&[("home", &["Fix typo", "Buy milk"]), ("work", &["Review"])]),
            crate::config::defaults(),
        );
        app.sections[1].tasks[0].body = "fix it".to_string();
        app.hide_preview = true;
        app.update(Action::Filter);
        for c in "fix".chars() {
            app.update(Action::FilterType(c));
        }
        let mut terminal = Terminal::new(TestBackend::new(50, 9)).unwrap();
        terminal
            .draw(|frame| {
                render(&app, frame);
            })
            .unwrap();
        let buffer = terminal.backend().buffer();
        let lines: Vec<String> = screen(&app).lines().map(str::to_string).collect();

        assert!(
            lines[2].trim_end().ends_with(" 1"),
            "only shown tasks count"
        );
        // `• Fix typo` starts at column 5, the match in bold yellow, the rest plain.
        assert!(lines[3].contains("• Fix typo"), "{lines:#?}");
        assert_eq!(buffer[(7, 3)].style().fg, Some(Color::Yellow));
        assert!(buffer[(7, 3)].modifier.contains(Modifier::BOLD));
        assert_eq!(buffer[(11, 3)].style().fg, Some(Color::Reset));
        // Matched in its body only: dim, with a hint.
        assert!(lines[6].contains("• Review …"), "{lines:#?}");
        assert!(buffer[(7, 6)].modifier.contains(Modifier::DIM));
    }
}
