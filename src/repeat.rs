//! Repeating tasks: a rule as inline code at the end of the heading,
//! ``# Water plants `* * mon` ``. The day fields of cron (day of month, month, day of week),
//! nothing repeats more often than daily. When a task was last done is in the state file
//! ([`crate::state`]), not in the task. Pure logic.

use croner::{Cron, parser::CronParser};
use jiff::civil::{Date, Time};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Repeat {
    /// As written: `* * mon`.
    pub rule: String,
    cron: Cron,
}

/// Splits the rule off the end of `heading`: `Water plants `* * mon`` -> `("Water plants",
/// Some(..))`. Code that is no rule stays part of the title, so a typo shows in the list. So
/// does a heading that is only a rule, it would leave no title.
pub fn split(heading: &str) -> (&str, Option<Repeat>) {
    let rule = heading
        .trim_end()
        .strip_suffix('`')
        .and_then(|rest| rest.rsplit_once('`'))
        .filter(|(title, _)| !title.trim().is_empty())
        .and_then(|(title, rule)| Some((title.trim_end(), Repeat::parse(rule)?)));
    match rule {
        Some((title, repeat)) => (title, Some(repeat)),
        None => (heading, None),
    }
}

impl Repeat {
    /// Exactly the three day fields. With cron's five the minutes would be read as days.
    fn parse(rule: &str) -> Option<Self> {
        if rule.split_whitespace().count() != 3 {
            return None;
        }
        // Both day fields have to match: `1 * mon` is a Monday the 1st, not cron's surprising
        // "the 1st or any Monday".
        let parser = CronParser::builder().dom_and_dow(true).build();
        let cron = parser.parse(&format!("0 0 {rule}")).ok()?;
        Some(Self {
            rule: rule.to_string(),
            cron,
        })
    }

    /// Short words for the common rules, next to the title: `daily`, `weekdays`, `mon, thu`,
    /// `1st`, `1 jan`. Anything else as written, croner's descriptions are sentences.
    pub fn describe(&self) -> String {
        let rule = self.rule.to_lowercase();
        let fields: Vec<&str> = rule.split_whitespace().collect();
        let day: Option<u8> = fields[0].parse().ok();
        let month = match fields[1].parse::<usize>() {
            Ok(n) => MONTHS.get(n.wrapping_sub(1)).copied(),
            Err(_) => MONTHS.contains(&fields[1]).then_some(fields[1]),
        };
        let names = |field: &str| {
            field
                .chars()
                .all(|c| c.is_ascii_alphabetic() || ",-".contains(c))
        };
        match (fields[0], fields[1], fields[2]) {
            ("*", "*", "*") => "daily".to_string(),
            ("*", "*", "mon-fri") => "weekdays".to_string(),
            ("*", "*", "sat,sun" | "sun,sat" | "sat-sun") => "weekends".to_string(),
            ("*", "*", dow) if names(dow) => dow.replace(',', ", "),
            (_, "*", "*") if let Some(day) = day => ordinal(day),
            (_, _, "*") if let (Some(day), Some(month)) = (day, month) => format!("{day} {month}"),
            _ => self.rule.clone(),
        }
    }

    /// Due when a day of the rule came after `done`, up to `today`. A missed day stays due
    /// until it is done. Never done is due.
    pub fn due(&self, done: Option<Date>, today: Date) -> bool {
        let Some(done) = done else {
            return true;
        };
        self.cron
            .find_next_occurrence(&done.to_datetime(Time::midnight()), false)
            .is_ok_and(|next| next.date() <= today)
    }
}

const MONTHS: [&str; 12] = [
    "jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec",
];

/// `1st`, `2nd`, `3rd`, `11th`, `22nd`.
fn ordinal(n: u8) -> String {
    let suffix = match (n % 10, n % 100) {
        (_, 11..=13) => "th",
        (1, _) => "st",
        (2, _) => "nd",
        (3, _) => "rd",
        _ => "th",
    };
    format!("{n}{suffix}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn date(text: &str) -> Date {
        text.parse().unwrap()
    }

    fn rule(heading: &str) -> Repeat {
        split(heading).1.expect("a rule")
    }

    #[test]
    fn split_takes_only_a_rule_at_the_end() {
        let (title, repeat) = split("Water plants `* * mon`");
        assert_eq!(title, "Water plants");
        assert_eq!(repeat.unwrap().rule, "* * mon");
        assert_eq!(split("Water plants  `* * *`  ").0, "Water plants");

        for heading in [
            "Water plants",
            "Run `cargo test` first",
            "Water plants `0 9 * * mon`",
            "Water plants `* *`",
            "Water plants `* * someday`",
            "`* * *`",
        ] {
            assert_eq!(split(heading), (heading, None), "{heading}");
        }
    }

    #[test]
    fn describe_in_short_words() {
        for (heading, words) in [
            ("x `* * *`", "daily"),
            ("x `* * mon-fri`", "weekdays"),
            ("x `* * sat,sun`", "weekends"),
            ("x `* * thu`", "thu"),
            ("x `* * MON,thu`", "mon, thu"),
            ("x `1 * *`", "1st"),
            ("x `22 * *`", "22nd"),
            ("x `11 * *`", "11th"),
            ("x `1 1 *`", "1 jan"),
            ("x `24 dec *`", "24 dec"),
            // As written: numbers for days of the week, steps, both day fields.
            ("x `* * 1`", "* * 1"),
            ("x `*/2 * *`", "*/2 * *"),
            ("x `1 * mon`", "1 * mon"),
        ] {
            assert_eq!(rule(heading).describe(), words, "{heading}");
        }
    }

    #[test]
    fn due_once_a_day_of_the_rule_came() {
        // 2026-10-19 is a Monday.
        let daily = rule("x `* * *`");
        assert!(daily.due(None, date("2026-10-19")), "never done");
        assert!(!daily.due(Some(date("2026-10-19")), date("2026-10-19")));
        assert!(daily.due(Some(date("2026-10-18")), date("2026-10-19")));

        let monday = rule("x `* * mon`");
        assert!(
            !monday.due(Some(date("2026-10-19")), date("2026-10-25")),
            "Sunday"
        );
        assert!(monday.due(Some(date("2026-10-19")), date("2026-10-26")));
        assert!(
            monday.due(Some(date("2026-10-19")), date("2026-10-28")),
            "missed on Monday, still due on Wednesday"
        );

        let weekdays = rule("x `* * mon-fri`");
        assert!(
            !weekdays.due(Some(date("2026-10-23")), date("2026-10-25")),
            "weekend"
        );
        assert!(weekdays.due(Some(date("2026-10-23")), date("2026-10-26")));
    }

    #[test]
    fn both_day_fields_have_to_match() {
        // The next Monday the 1st after 2026-10-19 is 2027-02-01.
        let rule = rule("x `1 * mon`");
        assert!(
            !rule.due(Some(date("2026-10-19")), date("2026-11-01")),
            "a Sunday"
        );
        assert!(
            !rule.due(Some(date("2026-10-19")), date("2026-11-02")),
            "a Monday"
        );
        assert!(rule.due(Some(date("2026-10-19")), date("2027-02-01")));
    }
}
