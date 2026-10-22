//! The heatmap of the archive page: its columns, cells and shades. Pure, the days come from
//! [`crate::load::archive`].

use std::collections::BTreeMap;

use jiff::{ToSpan, civil::Date};

use crate::archive::Archive;

/// Tasks done per day, only days with an archive directory.
pub type Days = BTreeMap<Date, usize>;

/// Tasks done per day.
pub fn days(archive: &Archive) -> Days {
    archive
        .iter()
        .map(|(&day, done)| (day, done.len()))
        .collect()
}

fn monday(day: Date) -> Date {
    day - i64::from(day.weekday().to_monday_zero_offset()).days()
}

/// Cells in a heatmap column, a day each, so a column is a week.
pub const CELLS: i64 = 7;

/// A column of the heatmap: the week from `start`, a Monday.
#[derive(Debug, PartialEq, Eq)]
pub struct Column {
    pub start: Date,
}

impl Column {
    /// The day of a cell.
    pub fn day(&self, row: i64) -> Date {
        self.start + row.days()
    }
}

/// The column and row of the cell of `day`, `None` if no column has it.
pub fn find(columns: &[Column], day: Date) -> Option<(usize, i64)> {
    let first = columns.first()?.start;
    let days = i64::from((day - first).get_days());
    let column = usize::try_from(days.div_euclid(CELLS)).ok()?;
    (column < columns.len()).then_some((column, days.rem_euclid(CELLS)))
}

/// The day `columns` and `rows` away from `day`. With `skip` the next one with tasks done that
/// way, empty days are skipped. Stays if there is none before the edge or before today, without
/// a cell for `day` too.
pub fn step(
    columns: &[Column],
    days: &Days,
    day: Date,
    (columns_by, rows_by): (i64, i64),
    skip: bool,
    today: Date,
) -> Date {
    let Some((mut column, mut row)) = find(columns, day) else {
        return day;
    };
    loop {
        row += rows_by;
        let Some(next) = usize::try_from(column as i64 + columns_by)
            .ok()
            .filter(|&next| next < columns.len() && (0..CELLS).contains(&row))
        else {
            return day;
        };
        column = next;
        let cell = columns[column].day(row);
        if cell > today {
            return day;
        }
        if !skip || days.get(&cell).is_some_and(|&done| done > 0) {
            return cell;
        }
    }
}

/// The weeks from the first archived day to `today`, oldest first.
pub fn columns(days: &Days, today: Date) -> Vec<Column> {
    let first = days.keys().next().map_or(today, |&day| day.min(today));
    let mut start = monday(first);
    let mut columns = Vec::new();
    while start <= today {
        columns.push(Column { start });
        start += CELLS.days();
    }
    columns
}

/// The first of `visible` columns on screen out of `total`, with `right` of them right of the
/// screen in the last frame. Moves only as far as the `cursor` column needs to be on screen, like
/// the lists scroll.
pub fn first_visible(total: usize, visible: usize, cursor: Option<usize>, right: usize) -> usize {
    let last_start = total.saturating_sub(visible);
    let mut first = last_start.saturating_sub(right);
    if let Some(cursor) = cursor {
        first = first.min(cursor).max((cursor + 1).saturating_sub(visible));
    }
    first.min(last_start)
}

/// Where each of `steps` equal groups of `counts` ends, zeros left out, see [`level`]. Ranks
/// instead of a share of the maximum, so a single huge day does not wash out all others.
pub fn bounds(counts: impl Iterator<Item = usize>, steps: usize) -> Vec<usize> {
    let mut counts: Vec<usize> = counts.filter(|&count| count > 0).collect();
    counts.sort();
    // Ties go to the lower group, so a scale of mostly ones stays light.
    (1..steps)
        .map(|group| {
            let index = (counts.len() * group / steps).saturating_sub(1);
            counts.get(index).copied().unwrap_or_default()
        })
        .collect()
}

/// `0` for nothing, then `1` to `steps` for the group of the [`bounds`] a count is in.
pub fn level(count: usize, bounds: &[usize]) -> usize {
    if count == 0 {
        return 0;
    }
    1 + bounds.iter().filter(|&&end| count > end).count()
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::{archive::Done, task::Task};

    fn date(text: &str) -> Date {
        text.parse().unwrap()
    }

    #[test]
    fn columns_are_the_weeks_back_to_the_first_day() {
        // A Saturday, its week starts on 2026-10-12.
        let today = date("2026-10-17");
        let days = Days::from([(date("2026-09-30"), 1)]);
        let columns = columns(&days, today);
        let starts: Vec<String> = columns.iter().map(|c| c.start.to_string()).collect();
        assert_eq!(starts, ["2026-09-28", "2026-10-05", "2026-10-12"]);
        assert_eq!(columns.last().unwrap().day(5), today);
        // Years back, every week has its column.
        let days = Days::from([(date("2022-04-09"), 1)]);
        assert_eq!(super::columns(&days, today).len(), 237);
    }

    #[test]
    fn step_over_days() {
        // A Saturday: the last column is its week.
        let today = date("2026-10-17");
        let columns = [
            Column {
                start: date("2026-09-28"),
            },
            Column {
                start: date("2026-10-05"),
            },
            Column {
                start: date("2026-10-12"),
            },
        ];
        assert_eq!(find(&columns, today), Some((2, 5)));
        assert_eq!(find(&columns, date("2026-10-01")), Some((0, 3)));
        assert_eq!(find(&columns, date("2026-09-27")), None);
        assert_eq!(find(&columns, date("2026-10-19")), None);

        let walk = |day: &str, by, skip| {
            let some = Days::from([(date("2026-09-28"), 1), (date("2026-10-15"), 2)]);
            step(&columns, &some, date(day), by, skip, today).to_string()
        };
        let step = |day: &str, by| walk(day, by, false);
        assert_eq!(step("2026-10-13", (-1, 0)), "2026-10-06");
        assert_eq!(step("2026-10-06", (1, 0)), "2026-10-13");
        assert_eq!(step("2026-10-13", (0, 1)), "2026-10-14");
        // Edges and days after today: stays.
        assert_eq!(step("2026-10-01", (-1, 0)), "2026-10-01");
        assert_eq!(step("2026-10-12", (0, -1)), "2026-10-12");
        assert_eq!(step("2026-10-17", (0, 1)), "2026-10-17");
        assert_eq!(step("2026-10-11", (1, 0)), "2026-10-11");

        // Skipping goes past empty days to the next one with tasks that way, none: stays.
        let step = |day: &str, by| walk(day, by, true);
        assert_eq!(step("2026-10-12", (0, 1)), "2026-10-15");
        assert_eq!(step("2026-10-15", (0, -1)), "2026-10-15");
        assert_eq!(step("2026-10-12", (-1, 0)), "2026-09-28");
        assert_eq!(step("2026-10-15", (-1, 0)), "2026-10-15");
    }

    #[test]
    fn scrolls_only_as_far_as_the_cursor_needs() {
        // 10 columns, 4 on screen: from the right edge.
        assert_eq!(first_visible(10, 4, Some(9), 0), 6);
        assert_eq!(first_visible(10, 4, Some(6), 0), 6);
        // Past the left edge of the view it follows, then stays.
        assert_eq!(first_visible(10, 4, Some(5), 0), 5);
        assert_eq!(first_visible(10, 4, Some(7), 1), 5);
        assert_eq!(first_visible(10, 4, Some(9), 1), 6);
        // Everything fits.
        assert_eq!(first_visible(3, 4, Some(0), 0), 0);
        assert_eq!(first_visible(10, 4, None, 20), 0);
    }

    #[test]
    fn levels_by_equal_groups_of_the_busy_days() {
        let hundred = || (1..=100).chain([0, 0]);
        let ends = bounds(hundred(), 4);
        let levels: Vec<usize> = [0, 1, 25, 26, 50, 51, 75, 76, 100]
            .into_iter()
            .map(|count| level(count, &ends))
            .collect();
        assert_eq!(levels, [0, 1, 1, 2, 2, 3, 3, 4, 4]);
        assert_eq!(level(13, &bounds(hundred(), 8)), 2);
        // One huge day does not wash out the rest.
        assert_eq!(level(4, &bounds([1, 2, 3, 4, 50].into_iter(), 4)), 4);
        // Mostly ones stay light.
        assert_eq!(level(1, &bounds([1, 1, 1, 1, 1, 5].into_iter(), 4)), 1);
    }

    #[test]
    fn days_count_the_tasks_of_each_day() {
        let done = || Done {
            section: "work".to_string(),
            task: Task::parse(PathBuf::new(), ""),
        };
        let archive = Archive::from([
            (date("2026-10-12"), vec![done(), done(), done()]),
            (date("2026-10-13"), vec![done(), done()]),
        ]);
        assert_eq!(
            days(&archive),
            Days::from([(date("2026-10-12"), 3), (date("2026-10-13"), 2)])
        );
    }
}
