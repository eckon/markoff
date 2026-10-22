//! The archive page: the [heatmap](mod@heatmap) of done tasks, then the [list] of the tasks in
//! the selected day. The numbers come from [`crate::heatmap`], this only lays them out.

use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Stylize},
    text::Line,
    widgets::{Block, Padding, Paragraph},
};

mod heatmap;
mod list;

pub use list::range;

use super::LINE_COLOR;
use crate::app::{App, archive::Focus};

/// Lines between the heatmap and the list: a double line with a blank line on each side.
const SEPARATOR: usize = 3;

/// The heatmap from the top of `area`, the done tasks of the selected day below it in the rest,
/// a double line between them. The part without the keys is dimmed.
/// Returns the heatmap's offset, the list's offset and the column of the preview's divider.
pub fn render(app: &App, frame: &mut Frame, area: Rect) -> (usize, usize, Option<u16>) {
    // Only the part with the keys shows its cursor.
    let heatmap_keys = app.archive_page.focus == Focus::Heatmap;
    // The heatmap starts where the section headers of the list do.
    let block = Block::new().padding(Padding::new(2, 2, 0, 0));
    let inner = block.inner(area);
    let width = inner.width as usize;
    let heatmap = (!app.days.is_empty()).then(|| {
        heatmap::lines(
            &app.days,
            app.archive_matches().as_ref(),
            app.today,
            width,
            app.archive_page.heatmap_offset,
            (app.archive_day(), app.archive_page.anchor),
            heatmap_keys,
        )
    });
    let lines = match &heatmap {
        None => vec![Line::from("nothing archived yet".dim())],
        Some(heatmap) => heatmap.lines.clone(),
    };
    let height = (lines.len() as u16).min(area.height);
    let heatmap_area = Rect { height, ..area };
    frame.render_widget(Paragraph::new(lines).block(block.clone()), heatmap_area);
    let gap = Rect {
        y: heatmap_area.bottom(),
        height: (SEPARATOR as u16).min(area.height - height),
        ..area
    };
    let list_area = Rect {
        y: gap.bottom(),
        height: area.height - height - gap.height,
        ..area
    };
    // In the blank line between the heatmap and the double line.
    if let Some(heatmap) = &heatmap
        && gap.height > 0
    {
        let hints_area = Rect {
            height: 1,
            ..block.inner(gap)
        };
        frame.render_widget(heatmap.hints.clone(), hints_area);
    }
    let (offset, preview) = list::render(app, frame, list_area);

    // The cursor's `[` or the anchor's `(` in the first column has no gap before it, it goes into
    // the padding.
    for &(row, mark) in heatmap.iter().flat_map(|h| &h.first_column_marks) {
        // Below the row of labels.
        let y = inner.y + 1 + row as u16;
        if y < heatmap_area.bottom() {
            frame.buffer_mut()[(inner.x - 1, y)].set_symbol(mark);
        }
    }

    // The part without the keys is dimmed, a double line between the two.
    let dimmed = match app.archive_page.focus {
        Focus::Heatmap => list_area,
        Focus::List => heatmap_area,
    };
    frame.buffer_mut().set_style(dimmed, Modifier::DIM);
    let line = "═".repeat(width).fg(LINE_COLOR);
    let line_area = Rect {
        y: gap.y + 1,
        height: 1,
        ..block.inner(gap)
    };
    if gap.height == SEPARATOR as u16 {
        frame.render_widget(Line::from(line), line_area);
        // The preview's divider goes up into the double line.
        if let Some(preview) = preview {
            let buffer = frame.buffer_mut();
            buffer[(preview.x, line_area.y)].set_symbol("╤");
            buffer[(preview.x, line_area.y + 1)]
                .set_symbol("│")
                .set_fg(LINE_COLOR);
        }
    }
    (
        heatmap.map_or(0, |heatmap| heatmap.offset),
        offset,
        preview.map(|preview| preview.x),
    )
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use jiff::ToSpan;
    use ratatui::{Terminal, backend::TestBackend};

    use super::*;
    use crate::{
        app::tests::{TODAY, sections},
        keymap::Action,
        task::Task,
        ui::{CURRENT_COLOR, DONE_COLOR, render},
    };

    /// Renders the app 80 wide and returns the screen as lines.
    fn lines_80(app: &App, height: u16) -> Vec<String> {
        let mut terminal = Terminal::new(TestBackend::new(80, height)).unwrap();
        terminal
            .draw(|frame| {
                render(app, frame);
            })
            .unwrap();
        let buffer = terminal.backend().buffer();
        buffer
            .content()
            .chunks(buffer.area.width as usize)
            .map(|row| row.iter().map(|cell| cell.symbol()).collect())
            .collect()
    }

    #[test]
    fn one_cursor_and_the_selected_cell_in_the_current_color() {
        let mut app = App::new(PathBuf::new(), sections(&[("work", &[])]));
        app.today = TODAY;
        let done = crate::archive::Done {
            section: "work".to_string(),
            task: Task::parse(PathBuf::new(), "# Fix bug\n"),
        };
        app.archive = [(TODAY - 1.days(), vec![done])].into();
        app.days = crate::heatmap::days(&app.archive);
        app.update(Action::NextPage);
        let draw = |app: &App| {
            let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
            terminal
                .draw(|frame| {
                    render(app, frame);
                })
                .unwrap();
            terminal.backend().buffer().clone()
        };
        let text = |buffer: &ratatui::buffer::Buffer, y: u16| -> String {
            (0..80).map(|x| buffer[(x, y)].symbol()).collect()
        };
        // The heatmap's rows below its labels, the list's from the line below the separator.
        let heatmap = 3..10;
        let list = 13..22;

        let current = |buffer: &ratatui::buffer::Buffer| -> Vec<(u16, u16)> {
            let cells = heatmap.clone().flat_map(|y| (0..80).map(move |x| (x, y)));
            cells.filter(|&at| buffer[at].fg == CURRENT_COLOR).collect()
        };

        // The heatmap has the keys: the selected day in the current color between brackets. The
        // list has no cursor.
        let buffer = draw(&app);
        let selected_cell = current(&buffer);
        assert_eq!(
            selected_cell.len(),
            1,
            "only the selected cell is in the current color"
        );
        let (x, y) = selected_cell[0];
        assert_eq!(buffer[(x - 1, y)].symbol(), "[");
        assert_eq!(buffer[(x + 1, y)].symbol(), "]");
        assert!(list.clone().all(|y| !text(&buffer, y).contains('▌')));
        // One selected day: it is in the app header, not on the rows.
        assert!(
            list.clone()
                .any(|y| text(&buffer, y).trim_end() == "     • Fix bug")
        );

        // The list has the keys: its cursor shows, the brackets go, the selected cell keeps its
        // color.
        app.update(Action::FocusList);
        let buffer = draw(&app);
        assert_eq!(current(&buffer), selected_cell);
        assert!(heatmap.clone().all(|y| !text(&buffer, y).contains('[')));
        assert!(
            list.clone()
                .any(|y| text(&buffer, y).contains("▌   • Fix bug"))
        );
    }

    #[test]
    fn range_in_the_current_color_with_its_anchor_and_a_hint() {
        let mut app = App::new(PathBuf::new(), sections(&[("work", &[])]));
        app.today = TODAY;
        app.days = [(TODAY - 3.days(), 1), (TODAY, 1)].into();
        app.update(Action::NextPage);
        // From today, a Thursday, up to Monday: one column, today the anchor.
        app.update(Action::Visual);
        for _ in 0..3 {
            app.update(Action::Up);
        }
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        terminal
            .draw(|frame| {
                render(&app, frame);
            })
            .unwrap();
        let buffer = terminal.backend().buffer().clone();
        let line = |y: u16| -> String { (0..80).map(|x| buffer[(x, y)].symbol()).collect() };

        // Monday to Thursday in the current color, the cursor on Monday, the anchor on Thursday.
        let current: Vec<u16> = (3..10)
            .filter(|&y| (0..80).any(|x| buffer[(x, y)].fg == CURRENT_COLOR))
            .collect();
        assert_eq!(current, [3, 4, 5, 6]);
        assert!(line(3).contains("[░]"), "{}", line(3));
        assert!(line(6).contains("(░)"), "{}", line(6));
        assert!(
            line(23).ends_with("v / Esc end range  ? toggle help"),
            "{}",
            line(23)
        );
        // Not while the list has the keys, `v` does nothing there.
        app.update(Action::FocusList);
        let screen = lines_80(&app, 24);
        assert!(!screen[23].contains("v / Esc end range"), "{}", screen[23]);
    }

    #[test]
    fn filter_grays_cells_without_matches() {
        let mut app = App::new(PathBuf::new(), sections(&[("home", &[])]));
        app.today = TODAY;
        let done = |content: &str| crate::archive::Done {
            section: "home".to_string(),
            task: Task::parse(PathBuf::new(), content),
        };
        // A Monday, the first row of the heatmap, the selected cell is today.
        let monday = TODAY - 10.days();
        app.archive = [
            (monday, vec![done("# Fix bug\n")]),
            (TODAY, vec![done("# Water plants\n")]),
        ]
        .into();
        app.days = crate::heatmap::days(&app.archive);
        app.update(Action::NextPage);
        let draw = |app: &App| {
            let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
            terminal
                .draw(|frame| {
                    render(app, frame);
                })
                .unwrap();
            terminal.backend().buffer().clone()
        };
        let monday_color = |buffer: &ratatui::buffer::Buffer| {
            let x = (0..80)
                .find(|&x| "░▒▓█".contains(buffer[(x, 3)].symbol()))
                .unwrap();
            buffer[(x, 3)].fg
        };
        assert_eq!(monday_color(&draw(&app)), DONE_COLOR);

        app.update(Action::Filter);
        for c in "water".chars() {
            app.update(Action::FilterType(c));
        }
        app.update(Action::FilterAccept);
        let buffer = draw(&app);
        assert_eq!(monday_color(&buffer), LINE_COLOR);
        let footer: String = (0..80).map(|x| buffer[(x, 23)].symbol()).collect();
        assert!(
            footer.starts_with("── filter: water · 1 task ─"),
            "{footer:?}"
        );
    }

    #[test]
    fn preview_of_done_task_only_while_the_list_has_the_keys() {
        let mut app = App::new(PathBuf::new(), sections(&[("work", &[])]));
        app.today = TODAY;
        let done = |content: &str| crate::archive::Done {
            section: "work".to_string(),
            task: Task::parse(PathBuf::new(), content),
        };
        let long = "a done task with a title long enough to run past the split";
        let day = TODAY - 400.days();
        app.archive = [(
            day,
            vec![
                done("# Plain\n"),
                done(&format!("# {long}\n\n## Notes\nline")),
            ],
        )]
        .into();
        app.days = crate::heatmap::days(&app.archive);
        app.update(Action::NextPage);
        app.update(Action::FocusList);
        // `Plain` first, then the one with a body.
        app.update(Action::Down);

        let screen = lines_80(&app, 24);
        let at = |text: &str| screen.iter().position(|line| line.contains(text));
        let notes = at("## Notes").expect("preview");
        let divider = screen[notes].chars().position(|c| c == '│').unwrap();
        // The divider joins the double line above and the footer below.
        let double = at("═").unwrap();
        assert_eq!(
            screen[double].chars().nth(divider),
            Some('╤'),
            "{screen:#?}"
        );
        assert_eq!(screen[double + 1].chars().nth(divider), Some('│'));
        assert_eq!(screen[22].chars().nth(divider), Some('│'), "{screen:#?}");
        assert_eq!(screen[23].chars().nth(divider), Some('┴'), "{screen:#?}");
        // The list wraps at the split.
        let row = at("a done task").unwrap();
        let list = &screen[row][..screen[row].find('│').unwrap()];
        assert!(!list.contains("long enough"), "{screen:#?}");

        // The title gets all the width without the preview.
        let full_width = |app: &App| {
            let screen = lines_80(app, 24);
            assert!(
                !screen.iter().any(|line| line.contains("## Notes")),
                "{screen:#?}"
            );
            let row = screen
                .iter()
                .find(|line| line.contains("a done task"))
                .unwrap();
            assert!(row.contains("long enough"), "{screen:#?}");
        };
        // Not while the heatmap has the keys.
        app.update(Action::FocusHeatmap);
        full_width(&app);
        // Nor in the list after `p`.
        app.update(Action::FocusList);
        app.update(Action::Preview);
        full_width(&app);
    }

    #[test]
    fn heatmap_scrolls_and_counts_what_is_out_of_view() {
        let mut app = App::new(PathBuf::new(), sections(&[("work", &[])]));
        app.today = TODAY;
        // Far more weeks than fit, a Wednesday 400 days back.
        app.days = [(TODAY - 400.days(), 2), (TODAY, 1)].into();
        app.update(Action::NextPage);

        // On today, the newest weeks: the first one with its month and year, the two tasks done
        // before them counted below the heatmap.
        let screen = lines_80(&app, 24);
        assert!(screen[2].starts_with("  Feb 2026 "), "{screen:#?}");
        assert_eq!(screen[10].trim_end(), "  ← 2 more", "{screen:#?}");

        // `gg` scrolls to the first week, today's task is out of view on the right.
        app.update(Action::Top);
        let screen = lines_80(&app, 24);
        assert!(screen[2].starts_with("  Sep 2025 "), "{screen:#?}");
        assert!(!screen[10].contains('←'), "{screen:#?}");
        assert!(screen[10].trim_end().ends_with("1 more →"), "{screen:#?}");
    }

    #[test]
    fn small_screens_still_show_the_heatmap() {
        let mut app = App::new(PathBuf::new(), sections(&[("home", &["Fix typo"])]));
        app.today = TODAY;
        app.days = [(TODAY, 3)].into();
        app.update(Action::NextPage);
        for (width, height) in [(30, 30), (80, 12), (80, 6)] {
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            terminal
                .draw(|frame| {
                    render(&app, frame);
                })
                .unwrap();
            let screen = format!("{:?}", terminal.backend().buffer());
            assert!(screen.contains("Mon"), "{screen}");
        }
    }

    #[test]
    fn cursor_in_the_first_column_puts_its_bracket_in_the_padding() {
        let mut app = App::new(PathBuf::new(), sections(&[("work", &[])]));
        app.today = TODAY;
        let first = TODAY - 30.days();
        app.days = [(first, 1)].into();
        app.update(Action::NextPage);
        app.archive_page.day = Some(first);

        let screen = lines_80(&app, 24);
        let row = 3 + first.weekday().to_monday_zero_offset() as usize;
        let start: String = screen[row].chars().take(4).collect();
        assert!(
            start.starts_with(" [") && start.ends_with(']'),
            "{screen:#?}"
        );
    }

    #[test]
    fn archive_page() {
        let mut app = App::new(PathBuf::new(), sections(&[("home", &["Fix typo"])]));
        app.today = TODAY;
        let draw = |app: &App| {
            let mut terminal = Terminal::new(TestBackend::new(80, 40)).unwrap();
            terminal
                .draw(|frame| {
                    render(app, frame);
                })
                .unwrap();
            terminal.backend().buffer().clone()
        };
        let lines = |buffer: &ratatui::buffer::Buffer| {
            buffer
                .content()
                .chunks(buffer.area.width as usize)
                .map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>())
                .collect::<Vec<_>>()
        };
        // Today the busiest, older days so it is not in the first column.
        app.days = [(TODAY, 3), (TODAY - 10.days(), 1), (TODAY - 20.days(), 2)].into();
        app.hide_preview = true;

        // On the tasks page: the list, the footer as a line, today's done in the app header.
        let screen = lines(&draw(&app));
        assert!(screen[0].ends_with(" 1 open · ✓ 3 done"), "{screen:#?}");
        assert!(!screen.iter().any(|line| line.contains(" Mon")));
        assert_eq!(screen[38].trim(), "", "{screen:#?}");
        assert!(screen[39].starts_with("────"), "{screen:#?}");

        // On its page: the selected day in the app header, the heatmap from the top.
        app.update(Action::NextPage);
        let buffer = draw(&app);
        let screen = lines(&buffer);
        assert!(
            screen[0].starts_with(&format!(
                "── tasks  archive ── {} · today ─",
                TODAY.strftime("%a %d %b %Y")
            )),
            "{screen:#?}"
        );
        // Done in the selected cell, the test has no titles.
        assert!(screen[0].ends_with("─ ✓ 0 done"), "{screen:#?}");
        // Two spaces at the sides, a blank line below the heatmap.
        // The first column's month and year, `Oct` would not fit next to it.
        assert_eq!(screen[2].trim_end(), "  Sep 2026", "{screen:#?}");
        // The weekday label at the row's end, on a Monday right after the cursor's `]`.
        assert!(screen[3].trim_end().ends_with("Mon"), "{screen:#?}");
        assert_eq!(screen[10].trim(), "", "{screen:#?}");
        // Nothing done yet: today is selected, the one cell in the current color, between
        // brackets.
        let row = 3 + TODAY.weekday().to_monday_zero_offset() as u16;
        let current: Vec<u16> = (0..80)
            .filter(|&x| buffer[(x, row)].fg == CURRENT_COLOR)
            .collect();
        assert_eq!(current.len(), 1, "{screen:#?}");
        assert_eq!(buffer[(current[0], row)].symbol(), "█");
        assert!(screen[row as usize].contains("[█]"), "{screen:#?}");

        // Between heatmap and list a double line, the list without the keys is dimmed.
        assert_eq!(screen[11].trim(), "═".repeat(76), "{screen:#?}");
        assert_eq!(screen[12].trim(), "", "{screen:#?}");
        assert!(screen[13].contains("<empty>"), "{screen:#?}");
        assert!(buffer[(2, 13)].modifier.contains(Modifier::DIM));
        assert!(!buffer[(2, 3)].modifier.contains(Modifier::DIM));
        assert!(screen[39].ends_with("? toggle help"), "{screen:#?}");

        app.days.clear();
        let screen = lines(&draw(&app));
        assert!(screen[2].contains("nothing archived yet"), "{screen:#?}");
    }
}
