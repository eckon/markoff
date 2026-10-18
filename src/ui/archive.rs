//! The archive view (`a`): the [`heatmap`] of done tasks from the bottom over the list, and the
//! [`stats`] below it. The numbers come from [`crate::history`], this only lays them out.

use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Stylize},
    text::Line,
    widgets::{Block, BorderType, Clear, Padding, Paragraph},
};

mod heatmap;
mod stats;

use super::{DONE_COLOR, LINE_COLOR};
use crate::app::App;
use stats::{Stats, stats};

/// The heatmap in a frame from the bottom of `area`, the rest dimmed behind it like for the
/// help. The status line stays as it is.
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
    let behind = Rect {
        height: frame.area().height.saturating_sub(1),
        ..frame.area()
    };
    frame.buffer_mut().set_style(behind, Modifier::DIM);
    let chrome = area.height - inner.height;
    let height = (lines.len() as u16 + chrome).min(area.height);
    let panel = Rect::new(area.x, area.bottom() - height, area.width, height);
    // Inside the border, and inside the padding too.
    let framed = block.clone().padding(Padding::ZERO).inner(panel);
    let inner = block.inner(panel);
    frame.render_widget(Clear, panel);
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
        app.update(Action::Archive);
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
    fn archive_from_the_bottom_when_toggled() {
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

        // Open: the frame from the bottom with its title, the heatmap, then the stats.
        app.section_days = [("home".to_string(), app.days.clone())].into();
        app.update(Action::Archive);
        let buffer = draw(&app);
        let screen = lines(&buffer);
        assert!(screen[0].ends_with(" 1 open · ✓ 3 done"), "{screen:#?}");
        assert!(screen[5].starts_with("╭ archive ─"), "{screen:#?}");
        assert!(
            screen[5].ends_with("─╮"),
            "no numbers in the title\n{screen:#?}"
        );
        // Room around the heatmap: a blank line above and below, two spaces at the sides.
        assert_eq!(screen[6].trim_matches(['│', ' ']), "", "{screen:#?}");
        assert!(screen[7].starts_with("│  daily"), "{screen:#?}");
        assert!(screen[8].contains(" Mon "), "{screen:#?}");
        assert_eq!(screen[15].trim_matches(['│', ' ']), "", "{screen:#?}");
        // Today is the one blue cell, without brackets.
        let row = 8 + today.weekday().to_monday_zero_offset() as u16;
        let blue: Vec<u16> = (0..80)
            .filter(|&x| buffer[(x, row)].fg == HEADER_COLOR)
            .collect();
        assert_eq!(blue.len(), 1, "{screen:#?}");
        assert_eq!(buffer[(blue[0], row)].symbol(), "█");
        assert!(!screen[row as usize].contains('['), "{screen:#?}");

        // The table with lines between its columns, joined where they meet the others, a
        // double line to the years, both with a heading and a line below it.
        assert!(
            screen[16].starts_with("├───────────┬───────┬───────┬───────┬─────────╥─"),
            "{screen:#?}"
        );
        assert!(
            screen[17].starts_with("│           │  week │ month │  year │all time ║ years "),
            "{screen:#?}"
        );
        assert!(
            screen[18].starts_with("├───────────┼───────┼───────┼───────┼─────────╫─"),
            "{screen:#?}"
        );
        assert!(screen[18].ends_with("─┤"), "{screen:#?}");
        assert!(
            screen[19].starts_with("│  done     │     3 "),
            "{screen:#?}"
        );
        assert!(
            screen[19].contains(&format!("║ {} ", today.year())),
            "{screen:#?}"
        );
        assert!(
            screen[20].contains("│       – ║"),
            "no previous for all time\n{screen:#?}"
        );
        assert!(
            screen[22].starts_with("│  best day │     3 "),
            "{screen:#?}"
        );
        // A line closes the table, then the share of each section per period.
        assert!(
            screen[23].starts_with("├───────────┴───────┴───────┴───────┴─────────╫─"),
            "{screen:#?}"
        );
        assert!(screen[23].ends_with("─┤"), "{screen:#?}");
        // The text left of the double line, the bar right of it where the year bars start.
        assert!(
            screen[24].starts_with("│  week     home 100% "),
            "{screen:#?}"
        );
        assert!(screen[24].contains("║      █"), "{screen:#?}");
        // The tasks of the shares where the years have their numbers.
        let column = |line: &str, text: &str| line[..line.find(text).unwrap()].chars().count();
        assert_eq!(
            column(&screen[24], " 3 "),
            column(&screen[19], " 6 so far"),
            "{screen:#?}"
        );
        assert!(
            screen[27].starts_with("│  all time home 100% "),
            "{screen:#?}"
        );
        assert!(
            screen[28].starts_with("╰─────────────────────────────────────────────╨─"),
            "{screen:#?}"
        );
        assert!(screen[29].ends_with("a toggle archive"), "{screen:#?}");

        app.days.clear();
        let screen = lines(&draw(&app));
        assert!(screen[26].contains("nothing archived yet"), "{screen:#?}");
    }
}
