//! The archive view (`a`): the heatmap of done tasks from the bottom over the list, and the
//! stats below it. The numbers come from [`history`], this only lays them out.

use jiff::civil::Date;
use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, BorderType, Clear, Padding, Paragraph},
};

use super::{DONE_COLOR, HEADER_COLOR, LINE_COLOR, section_color, spans};
use crate::{app::App, history};

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
        false => (heatmap_lines(&app.days, width), stats(app, width)),
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

/// The stats below the heatmap and where their lines go, so the frame can join them.
#[derive(Default)]
struct Stats {
    lines: Vec<Line<'static>>,
    /// Rows that are lines, joined to the frame on the left, and on the right if `true`.
    rules: Vec<(usize, bool)>,
    /// Lines down from the line above the stats to the row `end`, as their column and where
    /// they meet those two. With a third, they go on to the frame and meet it there.
    verticals: Vec<(usize, &'static str, &'static str, Option<&'static str>)>,
    end: usize,
}

/// The sections in list order, each with its color and days, and the days of the ones gone
/// from the list together, see [`history::Sections`].
fn sections(app: &App) -> (Vec<(String, Style, history::Days)>, history::Days) {
    let sections: Vec<(String, Style, history::Days)> = app
        .sections
        .iter()
        .enumerate()
        .map(|(i, section)| {
            let days = app
                .section_days
                .get(&section.name)
                .cloned()
                .unwrap_or_default();
            (
                section.name.clone(),
                Style::new().fg(section_color(i)),
                days,
            )
        })
        .collect();
    let mut gone = history::Days::new();
    for (name, days) in &app.section_days {
        if !app.sections.iter().any(|section| &section.name == name) {
            for (&day, &done) in days {
                *gone.entry(day).or_default() += done;
            }
        }
    }
    (sections, gone)
}

/// The period table on the left and the years on the right, if there is room for them, with
/// a double line between. Below a line, a bar per period split by the sections.
fn stats(app: &App, width: usize) -> Stats {
    /// Least width of each period's numbers, so the columns don't change with every digit.
    const MIN_NUMBER_WIDTH: usize = 6;
    /// Least room for the years next to the table: a year, a short bar and its number.
    /// Below it the shares go under the table instead.
    const MIN_YEARS_WIDTH: usize = 20;
    /// Longest share bar without the years, so it doesn't stretch over a wide screen.
    const MAX_SHARE_BAR: usize = 40;

    let days = &app.days;
    let today = jiff::Zoned::now().date();
    let periods = history::periods(days, today);
    let numbers: [NumberRow; 4] = [
        ("done", |period| period.done.to_string()),
        ("previous", |period| {
            period
                .previous
                .map_or("–".to_string(), |done| done.to_string())
        }),
        ("per day", |period| format!("{:.1}", period.per_day)),
        ("best day", |period| period.best_day.to_string()),
    ];
    // The row names here and the period names in the shares below, then a space.
    let label_width = numbers
        .iter()
        .map(|(label, _)| label.chars().count())
        .chain(periods.iter().map(|period| period.name.chars().count()))
        .max()
        .unwrap_or(0)
        + 1;
    // Each period as wide as its name and its numbers, at least [`MIN_NUMBER_WIDTH`].
    let widths: Vec<usize> = periods
        .iter()
        .map(|period| {
            numbers
                .iter()
                .map(|(_, cell)| cell(period).chars().count())
                .chain([period.name.chars().count(), MIN_NUMBER_WIDTH])
                .max()
                .unwrap_or(0)
        })
        .collect();
    // The row names, then per period a line, its numbers and a space.
    let table_width = label_width + widths.iter().map(|width| width + 2).sum::<usize>();
    // The divider and a space after it, the table ends in a space.
    let years_width = width.saturating_sub(table_width + 2);
    let split = years_width >= MIN_YEARS_WIDTH;

    // A column per period, a row per number, so each row says what it shows. Thin lines
    // between the columns keep the numbers apart.
    let row = |label: &str, cell: fn(&history::Period) -> String, header: bool| {
        let mut spans = vec![format!("{label:<label_width$}").into()];
        for (period, &width) in periods.iter().zip(&widths) {
            let text = format!("{:>width$} ", cell(period));
            spans.push("│".fg(LINE_COLOR));
            spans.push(if header { text.dim() } else { text.into() });
        }
        spans
    };
    let mut rows = vec![row("", |period| period.name.to_string(), true)];
    // A line under the names of the periods, crossing the lines between them.
    let underline_at = rows.len();
    let mut underline = vec!["─".repeat(label_width).fg(LINE_COLOR)];
    for width in &widths {
        underline.push(format!("┼{}", "─".repeat(width + 1)).fg(LINE_COLOR));
    }
    rows.push(underline);
    rows.extend(numbers.iter().map(|(label, cell)| row(label, *cell, false)));
    // Next to the table's heading and its line, the years have the same, so the bars start
    // next to the numbers.
    let mut years = vec![
        vec!["years".dim()],
        vec!["─".repeat(years_width).fg(LINE_COLOR)],
    ];
    let (year_bars, bar_width) = year_bars(
        &history::years(days, today),
        today,
        years_width,
        numbers.len(),
    );
    years.extend(year_bars);
    let mut lines: Vec<Line> = rows
        .into_iter()
        .enumerate()
        .map(|(i, mut spans)| {
            // A double line, the years are not part of the table.
            if split {
                spans.push(if i == underline_at { "╫─" } else { "║ " }.fg(LINE_COLOR));
                spans.extend(years.get(i).cloned().unwrap_or_default());
            }
            Line::from(spans)
        })
        .collect();
    // Where the lines between the columns of the table are.
    let mut verticals: Vec<(usize, &str, &str, Option<&str>)> = widths
        .iter()
        .scan(label_width, |x, width| {
            let at = *x;
            *x += width + 2;
            Some((at, "┬", "┴", None))
        })
        .collect();
    // The double line goes on through the shares below, to the frame.
    if split {
        verticals.push((table_width, "╥", "╫", Some("╨")));
    }

    // The table and the years end in a line, then the share of each section per period.
    let end = lines.len();
    lines.push(Line::from("─".repeat(width).fg(LINE_COLOR)));
    let (sections, gone) = sections(app);
    let sections: Vec<(String, Style, Vec<history::Period>)> = sections
        .into_iter()
        .map(|(name, style, section_days)| (name, style, history::periods(&section_days, today)))
        .collect();
    let gone = history::periods(&gone, today);
    for (i, period) in periods.iter().enumerate() {
        let shares: Vec<Share> = sections
            .iter()
            .map(|(name, style, periods)| (name.clone(), *style, periods[i].done))
            .collect();
        let mut spans = vec![format!("{:<label_width$}", period.name).dim()];
        let Some((parts, total)) = merge_small(&shares, gone[i].done) else {
            spans.push("nothing done".dim());
            lines.push(Line::from(spans));
            continue;
        };
        // Like the rows above: the text left of the double line, the bar right of it, where
        // and as long as the year bars are.
        if split {
            let (legend, named) = share_legend(&parts, total, table_width - label_width - 1);
            let used: usize = legend.iter().map(Span::width).sum();
            spans.extend(legend);
            spans.push(" ".repeat(table_width - label_width - used).into());
            spans.push("║ ".fg(LINE_COLOR));
            spans.push(" ".repeat(YEAR_LABEL).into());
            // The tasks of the named shares where the years have their numbers. Where they
            // need more room, the bar gets shorter.
            let counts = share_counts(&parts, &named);
            let counts_width: usize = counts.iter().map(Span::width).sum();
            let room = years_width.saturating_sub(YEAR_LABEL + 1 + counts_width);
            spans.extend(share_bar(&parts, total, bar_width.min(room)));
            spans.push(" ".into());
            spans.extend(counts);
        } else {
            let bar_width = (width.saturating_sub(label_width) / 2).min(MAX_SHARE_BAR);
            spans.extend(share_bar(&parts, total, bar_width));
            spans.push("  ".into());
            spans.extend(share_legend(&parts, total, usize::MAX).0);
        }
        lines.push(Line::from(spans));
    }
    Stats {
        lines,
        rules: vec![(underline_at, split), (end, true)],
        verticals,
        end,
    }
}

/// A row of numbers in the period table: its name and its number for a period.
type NumberRow = (&'static str, fn(&history::Period) -> String);

/// The year before each year bar and its space.
const YEAR_LABEL: usize = 5;

/// A section's name, color and tasks done in a period, for the share bars.
type Share = (String, Style, usize);

/// Sections below this share of a period are merged into `other` in the share bars.
const SMALL_SHARE: f64 = 0.05;

/// The sections' shares, the biggest first, with the small ones and the tasks of sections gone
/// from the list merged into a dim `other` at the end, and the total. The bar, the text and the
/// counts all go in this order, so a row reads the same from left to right. `None` if nothing
/// was done.
fn merge_small(shares: &[Share], gone: usize) -> Option<(Vec<Share>, usize)> {
    let total = shares.iter().map(|(_, _, done)| done).sum::<usize>() + gone;
    if total == 0 {
        return None;
    }
    let mut parts: Vec<Share> = Vec::new();
    let mut small = gone;
    for (name, style, done) in shares {
        if (*done as f64) < total as f64 * SMALL_SHARE {
            small += done;
        } else {
            parts.push((name.clone(), *style, *done));
        }
    }
    parts.sort_by_key(|(_, _, done)| std::cmp::Reverse(*done));
    if small > 0 {
        parts.push(("other".to_string(), Style::new().fg(LINE_COLOR), small));
    }
    Some((parts, total))
}

/// One bar of `width` split into the shares in their colors.
fn share_bar(parts: &[Share], total: usize, width: usize) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    let (mut so_far, mut drawn) = (0, 0);
    for (_, style, done) in parts {
        so_far += done;
        let end = so_far * width / total;
        spans.push(Span::styled("█".repeat(end - drawn), *style));
        drawn = end;
    }
    spans
}

/// The tasks of the `named` shares in their colors: `14+3+1`.
fn share_counts(parts: &[Share], named: &[String]) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    for (_, style, done) in parts.iter().filter(|(name, _, _)| named.contains(name)) {
        if !spans.is_empty() {
            spans.push("+".dim());
        }
        spans.push(Span::styled(done.to_string(), *style));
    }
    spans
}

/// The shares as text: `work 83%  misc 11%`, as many as fit in `width`, `…` for the rest. The
/// colors keep the names apart. Also the names it shows.
fn share_legend(parts: &[Share], total: usize, width: usize) -> (Vec<Span<'static>>, Vec<String>) {
    let mut spans = Vec::new();
    let mut named = Vec::new();
    let mut used = 0;
    let count = parts.len();
    for (i, (name, style, done)) in parts.iter().enumerate() {
        // A share that rounds down to nothing is still there.
        let percent = match done * 100 / total {
            0 => "<1".to_string(),
            percent => percent.to_string(),
        };
        let gap = if i == 0 { 0 } else { 2 };
        let percent = format!(" {percent}%");
        let item = gap + name.chars().count() + percent.chars().count();
        // Room for `…` unless it is the last one.
        let rest = if i + 1 == count { 0 } else { 1 };
        if used + item + rest > width {
            spans.push("…".dim());
            break;
        }
        if gap > 0 {
            spans.push("  ".into());
        }
        spans.push(Span::styled(name.clone(), *style));
        spans.push(percent.dim());
        named.push(name.clone());
        used += item;
    }
    (spans, named)
}

/// A bar per year, newest on top, as long as its tasks done, in `rows` lines: with more years
/// the last line sums up the rest. This year goes on dimmed to where it ends at the pace so far.
/// Also how long the bars can get.
fn year_bars(
    years: &[(i16, usize)],
    today: Date,
    width: usize,
    rows: usize,
) -> (Vec<Vec<Span<'static>>>, usize) {
    const EIGHTHS: [&str; 8] = ["", "▏", "▎", "▍", "▌", "▋", "▊", "▉"];
    /// After this year's number, its bar runs on to the projection.
    const SO_FAR: &str = " so far";
    let (shown, rest) = match years.len() > rows {
        true => years.split_at(rows - 1),
        false => (years, &[][..]),
    };
    let projected = |year: i16, done: usize| match year == today.year() {
        true => history::projected_year(done, today),
        false => done,
    };
    let most = shown
        .iter()
        .map(|&(year, done)| projected(year, done))
        .max()
        .unwrap_or(0)
        .max(1);
    let rest_done: usize = rest.iter().map(|(_, done)| done).sum();
    let digits = most.max(rest_done).to_string().len();
    // The year, a space after the bar, the number and [`SO_FAR`].
    let bar_width = width.saturating_sub(YEAR_LABEL + 1 + digits + SO_FAR.len());
    let eighths = |count: usize| count * bar_width * 8 / most;
    let mut lines: Vec<Vec<Span>> = shown
        .iter()
        .map(|&(year, done)| {
            let filled = eighths(done);
            let bar = "█".repeat(filled / 8) + EIGHTHS[filled % 8];
            let ahead = (eighths(projected(year, done)) / 8).saturating_sub(filled.div_ceil(8));
            let mut spans = vec![
                format!("{year} ").dim(),
                bar.fg(DONE_COLOR),
                "░".repeat(ahead).fg(DONE_COLOR).dim(),
                " ".repeat(bar_width.saturating_sub(filled.div_ceil(8) + ahead) + 1)
                    .into(),
                format!("{done:>digits$}").into(),
            ];
            if year == today.year() {
                spans.push(SO_FAR.dim());
            }
            spans
        })
        .collect();
    // Years come newest first, so the rest runs from its last to its first.
    if let (Some((newest, _)), Some((oldest, _))) = (rest.first(), rest.last()) {
        let label = format!("{oldest}–{newest}");
        // Up to where the numbers of the bars start.
        let pad = (YEAR_LABEL + bar_width + 1).saturating_sub(label.chars().count());
        lines.push(vec![
            label.dim(),
            " ".repeat(pad).into(),
            format!("{rest_done:>digits$}").into(),
        ]);
    }
    (lines, bar_width)
}

/// The frame of the archive, its title says what it is, the numbers are in the stats.
fn archive_block() -> Block<'static> {
    Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(LINE_COLOR)
        .padding(Padding::horizontal(2))
        .title(Line::from(" archive ".fg(DONE_COLOR).bold()))
}

/// Room for the weekdays on the right, next to the day cells they belong to.
const WEEKDAYS: usize = 4;

/// A cell and the space after it.
const STEP: usize = 2;

/// The glyph per step from little to a lot, see [`history::bounds`]. Only the shade changes:
/// fonts draw it brighter in order, while dim, light and bold greens often look the same.
fn steps() -> [Span<'static>; 4] {
    ["░", "▒", "▓", "█"].map(|shade| shade.fg(DONE_COLOR))
}

/// The tasks done over time, back to the first archived day, see [`history::columns`]: labels,
/// 7 rows of cells with today in blue and the weekdays on the right where cells are days, and
/// a divider through the gap where the cell size changes.
fn heatmap_lines(days: &history::Days, width: usize) -> Vec<Line<'static>> {
    let today = jiff::Zoned::now().date();
    let room = width.saturating_sub(WEEKDAYS);
    let columns = history::columns(days, today, room / STEP);
    let steps = steps();
    let bounds = history::bounds(
        columns.iter().flat_map(|column| {
            (0..history::CELLS).map(|row| history::rate(days, column.cell(row)))
        }),
        steps.len(),
    );
    // Weekdays only line up for day cells, which are on the right.
    let day_cells = columns.last().is_some_and(|column| column.unit == 1);

    let zone_starts = zone_starts(&columns);
    let mut lines = vec![labels(&columns, &zone_starts)];
    for row in 0..history::CELLS {
        let mut spans: Vec<Span> = Vec::new();
        for (i, column) in columns.iter().enumerate() {
            let cell = column.cell(row);
            let gap: Span = match zone_starts.get(i + 1) {
                Some(true) => "│".fg(LINE_COLOR),
                _ => " ".into(),
            };
            if cell.0 > today {
                spans.extend([" ".into(), gap]);
                continue;
            }
            let shade = match history::level(history::rate(days, cell), &bounds) {
                0 => "·".fg(LINE_COLOR),
                level => steps[level - 1].clone(),
            };
            // Today in the header's blue.
            let shade = if cell.0 == today {
                shade.fg(HEADER_COLOR)
            } else {
                shade
            };
            spans.extend([shade, gap]);
        }
        // Every other weekday, named after the day of its cell.
        let weekday = match columns.last() {
            Some(last) if day_cells && matches!(row, 0 | 2 | 4) => {
                last.cell(row).0.strftime("%a").to_string()
            }
            _ => String::new(),
        };
        spans.push(weekday.dim());
        lines.push(Line::from(spans));
    }
    lines
}

/// What one cell covers, the name of a zone of columns.
fn zone(unit: i64) -> &'static str {
    match unit {
        1 => "daily",
        2..=7 => "weekly",
        8..=31 => "monthly",
        _ => "yearly",
    }
}

/// Per column whether it starts another zone than the one before, a divider goes before it.
fn zone_starts(columns: &[history::Column]) -> Vec<bool> {
    let mut previous = None;
    columns
        .iter()
        .map(|column| {
            let starts = previous.is_some_and(|previous| previous != zone(column.unit));
            previous = Some(zone(column.unit));
            starts
        })
        .collect()
}

/// The row above the cells: the dividers, each zone's name after it, and where there is room
/// months where cells are days and years elsewhere, at the column they start in.
fn labels(columns: &[history::Column], zone_starts: &[bool]) -> Line<'static> {
    // Dates may run into the weekdays on the right.
    let mut row = vec![(' ', Style::new()); columns.len() * STEP + WEEKDAYS];
    for (i, _) in zone_starts
        .iter()
        .enumerate()
        .filter(|(_, starts)| **starts)
    {
        row[i * STEP - 1] = ('│', Style::new().fg(LINE_COLOR));
    }
    let name = Style::new().dim().italic();
    for (i, column) in columns.iter().enumerate() {
        if i == 0 || zone_starts[i] {
            put(&mut row, i * STEP, zone(column.unit), name);
        }
    }
    let mut previous = None;
    for (i, column) in columns.iter().enumerate() {
        let label = match column.unit {
            1 => column.start.strftime("%b").to_string(),
            _ => column.start.year().to_string(),
        };
        // A month only where it starts, a week of the last one would push it out.
        let starts = column.unit > 1 || column.start.day() <= 7;
        if previous.as_ref() != Some(&label) && starts {
            put(&mut row, i * STEP, &label, Style::new().dim());
        }
        previous = Some(label);
    }
    Line::from(spans(&row))
}

/// Writes `text` into `row` at `x` if it is free there, with a space or a divider on both sides.
fn put(row: &mut [(char, Style)], x: usize, text: &str, style: Style) {
    let end = x + text.chars().count();
    let free = |at: usize| row.get(at).is_none_or(|(c, _)| *c == ' ');
    let edge = |at: usize| row.get(at).is_none_or(|(c, _)| *c == ' ' || *c == '│');
    if end <= row.len() && (x..end).all(free) && (x == 0 || edge(x - 1)) && edge(end) {
        for (i, c) in text.chars().enumerate() {
            row[x + i] = (c, style);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use jiff::ToSpan;
    use ratatui::{Terminal, backend::TestBackend};

    use super::*;
    use crate::{app::tests::sections, keymap::Action, ui::render};

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

    #[test]
    fn share_legend_names_what_fits_and_the_counts_follow() {
        let share = |name: &str, done| (name.to_string(), Style::new(), done);
        let shares = [share("misc", 1), share("home", 50), share("work", 149)];
        let text = |spans: &[Span]| {
            spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
        };

        // The biggest first, the small ones merged into `other` at the end.
        let (parts, total) = merge_small(&shares, 0).unwrap();
        let names: Vec<&str> = parts.iter().map(|(name, _, _)| name.as_str()).collect();
        assert_eq!((names, total), (vec!["work", "home", "other"], 200));

        // A share that rounds down to nothing still shows.
        let (legend, named) = share_legend(&parts, total, 40);
        assert_eq!(text(&legend), "work 74%  home 25%  other <1%");
        assert_eq!(text(&share_counts(&parts, &named)), "149+50+1");

        // Sections gone from the list are in the same `other`, there is only one.
        let (parts, _) = merge_small(&shares, 10).unwrap();
        let other: Vec<usize> = parts
            .iter()
            .filter(|(name, _, _)| name == "other")
            .map(|(_, _, done)| *done)
            .collect();
        assert_eq!(other, [11]);
        let (parts, total) = merge_small(&shares, 0).unwrap();

        // Too narrow for all: `…` for the rest, and only the named ones get their tasks.
        let (legend, named) = share_legend(&parts, total, 20);
        assert_eq!(text(&legend), "work 74%  home 25%…");
        assert_eq!(text(&share_counts(&parts, &named)), "149+50");
    }
}
