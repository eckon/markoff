//! The archive page: the [heatmap](mod@heatmap) of done tasks and the [stats](mod@stats) below
//! it, then the [list] of the tasks in the heatmap's cursor. The numbers come from
//! [`crate::history`], this only lays them out.

use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Stylize},
    text::Line,
    widgets::{Block, Padding, Paragraph},
};

mod heatmap;
mod list;
mod stats;

pub use list::range;

use super::LINE_COLOR;
use crate::{
    app::{App, archive::Focus},
    history,
};
use stats::{Stats, stats};

/// Least rows the list keeps below the overview, the stats give way first.
const MIN_LIST_ROWS: usize = 5;

/// Lines between the overview and the list: a double line with a blank line on each side.
const SEPARATOR: usize = 3;

/// The overview, heatmap and stats, from the top of `area`, the done tasks of the cursor's cell
/// below it in the rest, a double line between them. The part without the keys is dimmed.
/// Returns the heatmap's columns, the list's offset and the column of the preview's divider.
pub fn render(app: &App, frame: &mut Frame, area: Rect) -> (usize, usize, Option<u16>) {
    // Only the part with the keys shows its cursor.
    let heatmap_keys = app.archive.focus == Focus::Heatmap;
    // The heatmap starts where the section rules of the list do.
    let block = Block::new().padding(Padding::new(2, 2, 0, 0));
    let inner = block.inner(area);
    let width = inner.width as usize;
    let columns = heatmap::columns(width);
    let (mut lines, mut stats) = match app.days.is_empty() {
        true => (
            vec![Line::from("nothing archived yet".dim())],
            Stats::default(),
        ),
        false => (
            heatmap::lines(&app.days, width, app.archive_day(), heatmap_keys),
            stats(app, width),
        ),
    };
    // Below the overview the separator, then the list.
    let room = (inner.height as usize).saturating_sub(MIN_LIST_ROWS + SEPARATOR);
    if !app.archive.show_stats || lines.len() + 2 + stats.lines.len() > room {
        stats = Stats::default();
    }
    // A blank line and a line between the heatmap and the stats, the table's lines join it.
    let has_stats = !stats.lines.is_empty();
    let separator = lines.len() + 1;
    if has_stats {
        lines.push(Line::default());
        lines.push(Line::from("─".repeat(width).fg(LINE_COLOR)));
        lines.extend(stats.lines);
    }
    let height = (lines.len() as u16).min(area.height);
    let overview = Rect { height, ..area };
    frame.render_widget(Paragraph::new(lines).block(block.clone()), overview);
    let gap = Rect {
        y: overview.bottom(),
        height: (SEPARATOR as u16).min(area.height - height),
        ..area
    };
    let list_area = Rect {
        y: gap.bottom(),
        height: area.height - height - gap.height,
        ..area
    };
    let (offset, preview) = list::render(app, frame, list_area, columns);

    // The cursor's `[` in the first column has no gap before it, it goes into the padding.
    let today = jiff::Zoned::now().date();
    let cells = history::columns(&app.days, today, columns);
    if heatmap_keys
        && !app.days.is_empty()
        && let Some((0, row)) = history::find(&cells, app.archive_day())
    {
        // Below the row of labels.
        let y = inner.y + 1 + row as u16;
        if y < overview.bottom() {
            frame.buffer_mut()[(inner.x - 1, y)]
                .set_symbol("[")
                .set_style(Modifier::BOLD);
        }
    }

    if has_stats {
        let buffer = frame.buffer_mut();
        let top = inner.y + separator as u16;
        let end = top + 1 + stats.end as u16;
        for &(at, top_join, end_join) in &stats.verticals {
            let x = inner.x + at as u16;
            // The table does not shrink, on a narrow screen its lines run past the edge.
            if x >= inner.right() {
                continue;
            }
            buffer[(x, top)].set_symbol(top_join);
            if end < overview.bottom() {
                buffer[(x, end)].set_symbol(end_join);
            }
        }
    }

    // The part without the keys is dimmed, a double line between the two.
    let dimmed = match app.archive.focus {
        Focus::Heatmap => list_area,
        Focus::List => overview,
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
    (columns, offset, preview.map(|preview| preview.x))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use jiff::ToSpan;
    use ratatui::{Terminal, backend::TestBackend};

    use super::*;
    use crate::{
        app::tests::sections,
        keymap::Action,
        ui::{HEADER_COLOR, render},
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
    fn one_cursor_and_the_selection_in_blue() {
        let today = jiff::Zoned::now().date();
        let yesterday = today - 1.days();
        let mut app = App::new(PathBuf::new(), sections(&[("work", &[])]));
        let done = crate::archive::Done {
            section: "work".to_string(),
            title: "Fix bug".to_string(),
            body: String::new(),
        };
        app.done = [(yesterday, vec![done])].into();
        (app.days, app.section_days) = history::split(&app.done);
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
        let blue = |buffer: &ratatui::buffer::Buffer| -> Vec<(u16, u16)> {
            let cells = heatmap.clone().flat_map(|y| (0..80).map(move |x| (x, y)));
            cells.filter(|&at| buffer[at].fg == HEADER_COLOR).collect()
        };

        // The heatmap has the keys: the selected day blue between brackets, today not blue. The
        // list has no cursor.
        let buffer = draw(&app);
        let selected = blue(&buffer);
        assert_eq!(selected.len(), 1, "only the selection is blue");
        let (x, y) = selected[0];
        assert_eq!(buffer[(x - 1, y)].symbol(), "[");
        assert_eq!(buffer[(x + 1, y)].symbol(), "]");
        assert!(list.clone().all(|y| !text(&buffer, y).contains('▌')));

        // The list has the keys: its cursor shows, the brackets go, the selection stays blue.
        app.update(Action::Focus);
        let buffer = draw(&app);
        assert_eq!(blue(&buffer), selected);
        assert!(heatmap.clone().all(|y| !text(&buffer, y).contains('[')));
        assert!(
            list.clone()
                .any(|y| text(&buffer, y).contains("▌   • Fix bug"))
        );
    }

    #[test]
    fn preview_of_done_task_only_while_the_list_has_the_keys() {
        let today = jiff::Zoned::now().date();
        let mut app = App::new(PathBuf::new(), sections(&[("work", &[])]));
        let done = |title: &str, body: &str| crate::archive::Done {
            section: "work".to_string(),
            title: title.to_string(),
            body: body.to_string(),
        };
        let long = "a done task with a title long enough to run past the split";
        app.done = [(today, vec![done(long, "## Notes\nline"), done("Plain", "")])].into();
        (app.days, app.section_days) = history::split(&app.done);
        app.update(Action::NextPage);
        app.update(Action::Focus);
        // Newest first: `Plain`, then the one with a body.
        app.update(Action::Down);

        let screen = lines_80(&app, 24);
        let at = |text: &str| screen.iter().position(|line| line.contains(text));
        let notes = at("## Notes").expect("preview");
        let divider = screen[notes].chars().position(|c| c == '│').unwrap();
        // The divider joins the double line above and the rule above the status line.
        let double = at("═").unwrap();
        assert_eq!(
            screen[double].chars().nth(divider),
            Some('╤'),
            "{screen:#?}"
        );
        assert_eq!(screen[double + 1].chars().nth(divider), Some('│'));
        assert_eq!(screen[22].chars().nth(divider), Some('┴'), "{screen:#?}");
        // The list is cut at the split, the date at its end.
        let row = at("a done task").unwrap();
        assert!(screen[row][..screen[row].find('│').unwrap()].contains("today"));

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
            assert!(row.contains("past the split"), "{screen:#?}");
            assert!(row.trim_end().ends_with("today"), "{screen:#?}");
        };
        // Not while the heatmap has the keys.
        app.update(Action::Focus);
        full_width(&app);
        // Nor in the list after `p`.
        app.update(Action::Focus);
        app.update(Action::Preview);
        full_width(&app);
    }

    #[test]
    fn narrow_screen_cuts_the_table_off() {
        let today = jiff::Zoned::now().date();
        let mut app = App::new(PathBuf::new(), sections(&[("home", &["Fix typo"])]));
        app.days = [(today, 3)].into();
        app.update(Action::NextPage);
        let mut terminal = Terminal::new(TestBackend::new(30, 30)).unwrap();
        terminal
            .draw(|frame| {
                render(&app, frame);
            })
            .unwrap();
        let screen = format!("{:?}", terminal.backend().buffer());
        assert!(screen.contains("daily"), "{screen}");
    }

    #[test]
    fn archive_page() {
        let today = jiff::Zoned::now().date();
        let mut app = App::new(PathBuf::new(), sections(&[("home", &["Fix typo"])]));
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
        app.days = [(today, 3), (today - 10.days(), 1), (today - 20.days(), 2)].into();

        // On the todo page: the list, a rule above the status line, today's done in the header.
        let screen = lines(&draw(&app));
        assert!(screen[0].ends_with(" 1 open · ✓ 3 done"), "{screen:#?}");
        assert!(!screen.iter().any(|line| line.contains("daily")));
        assert_eq!(screen[38], "─".repeat(80), "{screen:#?}");

        // On its page: the selected day in the header, the heatmap from the top, the stats.
        app.section_days = [("home".to_string(), app.days.clone())].into();
        app.update(Action::NextPage);
        app.update(Action::Stats);
        let buffer = draw(&app);
        let screen = lines(&buffer);
        let date = today.strftime("%a %d %b %Y");
        assert!(
            screen[0].starts_with(&format!("── [ todo  archive ] ── {date} ─")),
            "{screen:#?}"
        );
        // Done in the selection, the test has no titles.
        assert!(screen[0].ends_with("─ ✓ 0 done"), "{screen:#?}");
        // Two spaces at the sides, a blank line below the heatmap.
        assert!(screen[2].starts_with("  daily"), "{screen:#?}");
        assert!(screen[3].contains(" Mon "), "{screen:#?}");
        assert_eq!(screen[10].trim(), "", "{screen:#?}");
        // Nothing done yet: the selection is today, the one blue cell, between brackets.
        let row = 3 + today.weekday().to_monday_zero_offset() as u16;
        let blue: Vec<u16> = (0..80)
            .filter(|&x| buffer[(x, row)].fg == HEADER_COLOR)
            .collect();
        assert_eq!(blue.len(), 1, "{screen:#?}");
        assert_eq!(buffer[(blue[0], row)].symbol(), "█");
        assert!(screen[row as usize].contains("[█]"), "{screen:#?}");
        assert!(!buffer[(blue[0], row)].modifier.contains(Modifier::REVERSED));

        // The table with lines between its columns, joined where they meet the others, a
        // double line to the years, both with a heading and a line below it.
        assert!(
            screen[11].starts_with("  ─────────┬───────┬───────┬───────┬─────────╥─"),
            "{screen:#?}"
        );
        assert!(
            screen[12].starts_with("           │  week │ month │  year │all time ║ years "),
            "{screen:#?}"
        );
        assert!(
            screen[13].starts_with("  ─────────┼───────┼───────┼───────┼─────────╫─"),
            "{screen:#?}"
        );
        assert!(screen[14].starts_with("  done     │     3 "), "{screen:#?}");
        assert!(
            screen[14].contains(&format!("║ {} ", today.year())),
            "{screen:#?}"
        );
        assert!(
            screen[15].contains("│       – ║"),
            "no previous for all time\n{screen:#?}"
        );
        assert!(screen[17].starts_with("  best day │     3 "), "{screen:#?}");
        // A line closes the table, then the share of each section per period.
        assert!(
            screen[18].starts_with("  ─────────┴───────┴───────┴───────┴─────────╫─"),
            "{screen:#?}"
        );
        // The text left of the double line, the bar right of it where the year bars start.
        assert!(
            screen[19].starts_with("  week     home 100% "),
            "{screen:#?}"
        );
        assert!(screen[19].contains("║      █"), "{screen:#?}");
        // The tasks of the shares where the years have their numbers.
        let column = |line: &str, text: &str| line[..line.find(text).unwrap()].chars().count();
        assert_eq!(
            column(&screen[19], " 3 "),
            column(&screen[14], " 6 so far"),
            "{screen:#?}"
        );
        assert!(
            screen[22].starts_with("  all time home 100% "),
            "{screen:#?}"
        );
        // Between overview and list a double line, the list without the keys is dimmed.
        assert_eq!(screen[23].trim(), "", "{screen:#?}");
        assert_eq!(screen[24].trim(), "═".repeat(76), "{screen:#?}");
        assert!(screen[26].contains("nothing done"), "{screen:#?}");
        assert!(buffer[(2, 26)].modifier.contains(Modifier::DIM));
        assert!(!buffer[(2, 14)].modifier.contains(Modifier::DIM));
        assert!(screen[39].ends_with("? toggle help"), "{screen:#?}");

        app.days.clear();
        let screen = lines(&draw(&app));
        assert!(screen[2].contains("nothing archived yet"), "{screen:#?}");
    }
}
