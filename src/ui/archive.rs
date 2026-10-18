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

use super::{HEADER_COLOR, LINE_COLOR};
use crate::app::{App, archive::Focus};
use stats::{Stats, stats};

/// Least rows the list keeps below the overview, the stats give way first.
const MIN_LIST_ROWS: usize = 5;

/// The overview, heatmap and stats, from the top of `area`, the done tasks of the cursor's cell
/// below it in the rest, a line between them says how to switch. The part without the keys is
/// dimmed. Returns the heatmap's columns and the list's offset.
pub fn render(app: &App, frame: &mut Frame, area: Rect) -> (usize, usize) {
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
            heatmap::lines(&app.days, width, app.archive_day()),
            stats(app, width),
        ),
    };
    // Below the overview the line with the hint, then the list.
    let room = (inner.height as usize).saturating_sub(MIN_LIST_ROWS + 1);
    if lines.len() + 1 + stats.lines.len() > room {
        stats = Stats::default();
    }
    // A blank line below the heatmap where there is room: the line and the stats come first.
    let stats_height = match stats.lines.len() {
        0 => 0,
        height => height + 1,
    };
    if lines.len() + stats_height < room {
        lines.push(Line::default());
    }
    // A line between the heatmap and the stats, the table's lines join it.
    let separator = lines.len();
    let has_stats = !stats.lines.is_empty();
    if has_stats {
        lines.push(Line::from("─".repeat(width).fg(LINE_COLOR)));
        lines.extend(stats.lines);
    }
    let height = (lines.len() as u16).min(area.height);
    let overview = Rect { height, ..area };
    frame.render_widget(Paragraph::new(lines).block(block.clone()), overview);
    let gap = Rect {
        y: overview.bottom(),
        height: 1.min(area.height - height),
        ..area
    };
    let list_area = Rect {
        y: gap.bottom(),
        height: area.height - height - gap.height,
        ..area
    };
    let offset = list::render(app, frame, list_area, columns);

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

    // What `Tab` moves the keys to, the other part dimmed.
    let (dimmed, to) = match app.archive.focus {
        Focus::Heatmap => (list_area, "list"),
        Focus::List => (overview, "heatmap"),
    };
    frame.buffer_mut().set_style(dimmed, Modifier::DIM);
    let hint = Line::from(vec!["Tab".bold().fg(HEADER_COLOR), format!(" {to}").dim()]);
    frame.render_widget(hint.right_aligned(), block.inner(gap));
    (columns, offset)
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
        let buffer = draw(&app);
        let screen = lines(&buffer);
        let date = today.strftime("%a %d %b %Y");
        assert!(
            screen[0].starts_with(&format!("── [ todo  archive ] ── {date} ─")),
            "{screen:#?}"
        );
        assert!(screen[0].ends_with(" 1 open · ✓ 3 done"), "{screen:#?}");
        // Two spaces at the sides, a blank line below the heatmap.
        assert!(screen[2].starts_with("  daily"), "{screen:#?}");
        assert!(screen[3].contains(" Mon "), "{screen:#?}");
        assert_eq!(screen[10].trim(), "", "{screen:#?}");
        // Today is the one blue cell, without brackets.
        let row = 3 + today.weekday().to_monday_zero_offset() as u16;
        let blue: Vec<u16> = (0..80)
            .filter(|&x| buffer[(x, row)].fg == HEADER_COLOR)
            .collect();
        assert_eq!(blue.len(), 1, "{screen:#?}");
        assert_eq!(buffer[(blue[0], row)].symbol(), "█");
        assert!(!screen[row as usize].contains('['), "{screen:#?}");

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
        // Between overview and list the key to the list, which is dimmed.
        assert!(screen[23].ends_with("Tab list  "), "{screen:#?}");
        assert!(screen[24].contains("nothing done"), "{screen:#?}");
        assert!(buffer[(2, 24)].modifier.contains(Modifier::DIM));
        assert!(!buffer[(2, 14)].modifier.contains(Modifier::DIM));
        assert!(screen[39].ends_with("? toggle help"), "{screen:#?}");

        app.days.clear();
        let screen = lines(&draw(&app));
        assert!(screen[2].contains("nothing archived yet"), "{screen:#?}");
    }
}
