//! The archive page: the [heatmap](mod@heatmap) of done tasks and the [stats](mod@stats) below
//! it. The numbers come from [`crate::history`], this only lays them out.

use ratatui::{
    Frame,
    layout::Rect,
    style::Stylize,
    text::Line,
    widgets::{Block, BorderType, Padding, Paragraph},
};

mod heatmap;
mod stats;

use super::{DONE_COLOR, LINE_COLOR};
use crate::app::App;
use stats::{Stats, stats};

/// The heatmap and the stats in a frame from the top of `area`.
pub fn render(app: &App, frame: &mut Frame, area: Rect) {
    let block = archive_block();
    let inner = block.inner(area);
    let width = inner.width as usize;
    let (mut lines, stats) = match app.days.is_empty() {
        true => (
            vec![Line::from("nothing archived yet".dim())],
            Stats::default(),
        ),
        false => (heatmap::lines(&app.days, width), stats(app, width)),
    };
    // A blank line above and below the heatmap where there is room: the separator and the
    // stats come on top.
    if lines.len() + 1 + stats.lines.len() + 2 <= inner.height as usize {
        lines.insert(0, Line::default());
        lines.push(Line::default());
    }
    // A line through the frame between the heatmap and the stats, drawn below.
    let separator = lines.len();
    let has_stats = !stats.lines.is_empty();
    if has_stats {
        lines.push(Line::default());
        lines.extend(stats.lines);
    }
    let chrome = area.height - inner.height;
    let height = (lines.len() as u16 + chrome).min(area.height);
    let panel = Rect { height, ..area };
    // Inside the border, and inside the padding too.
    let framed = block.clone().padding(Padding::ZERO).inner(panel);
    let inner = block.inner(panel);
    frame.render_widget(Paragraph::new(lines).block(block), panel);
    if !has_stats {
        return;
    }

    // Lines of the stats, joined to the frame and to each other.
    let buffer = frame.buffer_mut();
    let bottom = framed.bottom();
    let row = |at: usize| inner.y + (separator + 1 + at) as u16;
    let x = |at: usize| inner.x + at as u16;
    let top = inner.y + separator as u16;
    for at in framed.left()..framed.right() {
        buffer[(at, top)].set_symbol("─").set_fg(LINE_COLOR);
    }
    let mut rule = |y: u16, right: bool| {
        if y >= bottom {
            return;
        }
        buffer[(panel.x, y)].set_symbol("├");
        let ends = (framed.left()..inner.left()).chain(match right {
            true => inner.right()..framed.right(),
            false => 0..0,
        });
        for at in ends {
            buffer[(at, y)].set_symbol("─").set_fg(LINE_COLOR);
        }
        if right {
            buffer[(framed.right(), y)].set_symbol("┤");
        }
    };
    rule(top, true);
    for &(at, right) in &stats.rules {
        rule(row(at), right);
    }
    for &(at, top_join, end_join, bottom_join) in &stats.verticals {
        // The table does not shrink, on a narrow screen its lines run past the frame.
        if x(at) >= framed.right() {
            continue;
        }
        buffer[(x(at), top)].set_symbol(top_join);
        if row(stats.end) < bottom {
            buffer[(x(at), row(stats.end))].set_symbol(end_join);
        }
        if let Some(join) = bottom_join {
            buffer[(x(at), bottom)].set_symbol(join);
        }
    }
}

/// The frame of the archive, its title says what it is, the numbers are in the stats.
fn archive_block() -> Block<'static> {
    Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(LINE_COLOR)
        .padding(Padding::horizontal(2))
        .title(Line::from(" archive ".fg(DONE_COLOR).bold()))
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
        assert!(screen.contains("╭ archive"), "{screen}");
    }

    #[test]
    fn archive_page() {
        let today = jiff::Zoned::now().date();
        let mut app = App::new(PathBuf::new(), sections(&[("home", &["Fix typo"])]));
        let draw = |app: &App| {
            let mut terminal = Terminal::new(TestBackend::new(80, 30)).unwrap();
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

        // Closed by default: the list, a rule above the status line, today's done in the header.
        let screen = lines(&draw(&app));
        assert!(screen[0].ends_with(" 1 open · ✓ 3 done"), "{screen:#?}");
        assert!(
            !screen.iter().any(|line| line.contains("╭ archive")),
            "{screen:#?}"
        );
        assert_eq!(screen[28], "─".repeat(80), "{screen:#?}");

        // On its page: the frame from the top with its title, the heatmap, then the stats.
        app.section_days = [("home".to_string(), app.days.clone())].into();
        app.update(Action::NextPage);
        let buffer = draw(&app);
        let screen = lines(&buffer);
        assert!(screen[0].ends_with(" 1 open · ✓ 3 done"), "{screen:#?}");
        assert!(screen[2].starts_with("╭ archive ─"), "{screen:#?}");
        assert!(
            screen[2].ends_with("─╮"),
            "no numbers in the title\n{screen:#?}"
        );
        // Room around the heatmap: a blank line above and below, two spaces at the sides.
        assert_eq!(screen[3].trim_matches(['│', ' ']), "", "{screen:#?}");
        assert!(screen[4].starts_with("│  daily"), "{screen:#?}");
        assert!(screen[5].contains(" Mon "), "{screen:#?}");
        assert_eq!(screen[12].trim_matches(['│', ' ']), "", "{screen:#?}");
        // Today is the one blue cell, without brackets.
        let row = 5 + today.weekday().to_monday_zero_offset() as u16;
        let blue: Vec<u16> = (0..80)
            .filter(|&x| buffer[(x, row)].fg == HEADER_COLOR)
            .collect();
        assert_eq!(blue.len(), 1, "{screen:#?}");
        assert_eq!(buffer[(blue[0], row)].symbol(), "█");
        assert!(!screen[row as usize].contains('['), "{screen:#?}");

        // The table with lines between its columns, joined where they meet the others, a
        // double line to the years, both with a heading and a line below it.
        assert!(
            screen[13].starts_with("├───────────┬───────┬───────┬───────┬─────────╥─"),
            "{screen:#?}"
        );
        assert!(
            screen[14].starts_with("│           │  week │ month │  year │all time ║ years "),
            "{screen:#?}"
        );
        assert!(
            screen[15].starts_with("├───────────┼───────┼───────┼───────┼─────────╫─"),
            "{screen:#?}"
        );
        assert!(screen[15].ends_with("─┤"), "{screen:#?}");
        assert!(
            screen[16].starts_with("│  done     │     3 "),
            "{screen:#?}"
        );
        assert!(
            screen[16].contains(&format!("║ {} ", today.year())),
            "{screen:#?}"
        );
        assert!(
            screen[17].contains("│       – ║"),
            "no previous for all time\n{screen:#?}"
        );
        assert!(
            screen[19].starts_with("│  best day │     3 "),
            "{screen:#?}"
        );
        // A line closes the table, then the share of each section per period.
        assert!(
            screen[20].starts_with("├───────────┴───────┴───────┴───────┴─────────╫─"),
            "{screen:#?}"
        );
        assert!(screen[20].ends_with("─┤"), "{screen:#?}");
        // The text left of the double line, the bar right of it where the year bars start.
        assert!(
            screen[21].starts_with("│  week     home 100% "),
            "{screen:#?}"
        );
        assert!(screen[21].contains("║      █"), "{screen:#?}");
        // The tasks of the shares where the years have their numbers.
        let column = |line: &str, text: &str| line[..line.find(text).unwrap()].chars().count();
        assert_eq!(
            column(&screen[21], " 3 "),
            column(&screen[16], " 6 so far"),
            "{screen:#?}"
        );
        assert!(
            screen[24].starts_with("│  all time home 100% "),
            "{screen:#?}"
        );
        assert!(
            screen[25].starts_with("╰─────────────────────────────────────────────╨─"),
            "{screen:#?}"
        );
        assert!(screen[29].ends_with("? toggle help"), "{screen:#?}");

        app.days.clear();
        let screen = lines(&draw(&app));
        assert!(screen[4].contains("nothing archived yet"), "{screen:#?}");
    }
}
