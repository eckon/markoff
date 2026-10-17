//! The archive over time, for the heatmap below the list. Pure, the days come from
//! [`crate::load::archive`].

use std::collections::BTreeMap;

use jiff::{ToSpan, civil::Date};

/// Tasks done per day, only days with an archive file.
pub type Days = BTreeMap<Date, usize>;

pub fn monday(day: Date) -> Date {
    day - i64::from(day.weekday().to_monday_zero_offset()).days()
}

/// A column of the heatmap: 7 cells of `unit` days each, from `start`.
#[derive(Debug, PartialEq, Eq)]
pub struct Column {
    pub start: Date,
    pub unit: i64,
}

impl Column {
    /// The first day of a cell and its days.
    pub fn cell(&self, row: i64) -> (Date, i64) {
        (self.start + (row * self.unit).days(), self.unit)
    }
}

/// Up to `count` columns from the week of `today` back to the first archived day, oldest
/// first. The cells grow by a constant factor from a day on the right, the smallest factor
/// that reaches back to the first day: recent days in detail, years back still there.
pub fn columns(days: &Days, today: Date, count: usize) -> Vec<Column> {
    let first = days.keys().next().map_or(today, |&day| day.min(today));
    let mut end = monday(today) + 7.days();
    let span = i64::from((end - first).get_days());
    let mut columns = Vec::new();
    for unit in units(count, span) {
        let start = end - (7 * unit).days();
        columns.push(Column { start, unit });
        end = start;
        if start <= first {
            break;
        }
    }
    columns.reverse();
    columns
}

/// Days per cell of `count` columns from the right, see [`columns`].
fn units(count: usize, span: i64) -> Vec<i64> {
    let sizes = |factor: f64| -> Vec<i64> {
        (0..count)
            // Capped, so a steep factor cannot overflow.
            .map(|i| (factor.powi(i as i32).floor() as i64).clamp(1, span.max(1)))
            .collect()
    };
    let reaches = |factor: f64| {
        let weeks = sizes(factor)
            .iter()
            .fold(0i64, |sum, unit| sum.saturating_add(*unit));
        weeks.saturating_mul(7) >= span
    };
    let (mut low, mut high) = (1.0, 3.0);
    for _ in 0..50 {
        let factor = (low + high) / 2.0;
        if reaches(factor) {
            high = factor;
        } else {
            low = factor;
        }
    }
    sizes(high)
}

/// Tasks done per day in a cell, so big cells compare with small ones.
pub fn rate(days: &Days, (start, length): (Date, i64)) -> f64 {
    let done: usize = days
        .range(start..start + length.days())
        .map(|(_, &done)| done)
        .sum();
    done as f64 / length as f64
}

/// Where each of `steps` equal groups of `values` ends, zeros left out, see [`level`]. Ranks
/// instead of a share of the maximum, so a single huge day does not wash out all others.
pub fn bounds<T: Copy + PartialOrd + Default>(
    values: impl Iterator<Item = T>,
    steps: usize,
) -> Vec<T> {
    let mut values: Vec<T> = values.filter(|&value| value > T::default()).collect();
    values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    // Ties go to the lower group, so a scale of mostly ones stays light.
    (1..steps)
        .map(|group| {
            let index = (values.len() * group / steps).saturating_sub(1);
            values.get(index).copied().unwrap_or_default()
        })
        .collect()
}

/// `0` for nothing, then `1` to `steps` for the group of the [`bounds`] a value is in.
pub fn level<T: Copy + PartialOrd + Default>(value: T, bounds: &[T]) -> usize {
    if value <= T::default() {
        return 0;
    }
    1 + bounds.iter().filter(|&&end| value > end).count()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn date(text: &str) -> Date {
        text.parse().unwrap()
    }

    #[test]
    fn columns_reach_back_to_the_first_day_with_days_on_the_right() {
        // A Saturday, its week starts on 2026-10-12.
        let today = date("2026-10-17");
        let days = Days::from([(date("2022-04-09"), 1)]);
        let columns = columns(&days, today, 40);
        assert_eq!(columns.len(), 40);
        let last = columns.last().unwrap();
        assert_eq!((last.start, last.unit), (date("2026-10-12"), 1));
        assert!(columns[0].start <= date("2022-04-09"), "{:?}", columns[0]);
        // Bigger cells further back, without gaps between the columns.
        for pair in columns.windows(2) {
            assert!(pair[0].unit >= pair[1].unit, "{pair:?}");
            assert_eq!(pair[0].start + (7 * pair[0].unit).days(), pair[1].start);
        }
    }

    #[test]
    fn short_history_takes_only_the_columns_it_needs() {
        let today = date("2026-10-17");
        let days = Days::from([(date("2026-09-30"), 1)]);
        let columns = columns(&days, today, 40);
        let starts: Vec<String> = columns.iter().map(|c| c.start.to_string()).collect();
        assert_eq!(starts, ["2026-09-28", "2026-10-05", "2026-10-12"]);
        assert!(columns.iter().all(|column| column.unit == 1));
        assert_eq!(columns.last().unwrap().cell(5).0, today);
    }

    #[test]
    fn rate_is_tasks_per_day_of_the_cell() {
        let days = Days::from([(date("2026-10-12"), 3), (date("2026-10-14"), 4)]);
        assert_eq!(rate(&days, (date("2026-10-12"), 1)), 3.0);
        assert_eq!(rate(&days, (date("2026-10-12"), 7)), 1.0);
        assert_eq!(rate(&days, (date("2026-10-13"), 1)), 0.0);
    }

    #[test]
    fn levels_by_equal_groups_of_the_busy_days() {
        let ends = bounds((1..=100).chain([0, 0]), 4);
        let levels: Vec<usize> = [0, 1, 25, 26, 50, 51, 75, 76, 100]
            .into_iter()
            .map(|count| level(count, &ends))
            .collect();
        assert_eq!(levels, [0, 1, 1, 2, 2, 3, 3, 4, 4]);
        assert_eq!(level(13, &bounds(1..=100, 8)), 2);
        // One huge day does not wash out the rest.
        assert_eq!(level(4, &bounds([1, 2, 3, 4, 50].into_iter(), 4)), 4);
        // Mostly ones stay light.
        assert_eq!(level(1, &bounds([1, 1, 1, 1, 1, 5].into_iter(), 4)), 1);
    }
}
