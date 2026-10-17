//! `daily.md`: a checklist that starts fresh every day. An add-on kept apart from `todo/`,
//! until recurring tasks get a real design. Pure string handling.
//!
//! Each `## ` heading is a task, the lines up to the next one its body. Text before the
//! first heading (a `# Daily` title) is ignored. Done marks the heading with today's date:
//! `## Book time in Jira [done 2026-10-17]`. An older date counts as open.

/// In the root directory, next to `todo/` and `archive/`.
pub const FILE: &str = "daily.md";

const MARKER_START: &str = " [done ";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DailyTask {
    pub title: String,
    pub body: String,
    /// Date of the marker, `YYYY-MM-DD`, `None` without one.
    pub done: Option<String>,
}

impl DailyTask {
    pub fn done_on(&self, today: &str) -> bool {
        self.done.as_deref() == Some(today)
    }
}

pub fn parse(content: &str) -> Vec<DailyTask> {
    let mut tasks: Vec<DailyTask> = Vec::new();
    let mut body = String::new();
    for line in content.lines() {
        if let Some(heading) = line.strip_prefix("## ") {
            if let Some(last) = tasks.last_mut() {
                last.body = body.trim().to_string();
            }
            body.clear();
            let (title, done) = split_marker(heading);
            tasks.push(DailyTask {
                title: title.to_string(),
                body: String::new(),
                done: done.map(str::to_string),
            });
        } else {
            body.push_str(line);
            body.push('\n');
        }
    }
    if let Some(last) = tasks.last_mut() {
        last.body = body.trim().to_string();
    }
    tasks
}

/// `X`: marks task `index` done `today`, or open again if it already is. Everything else in
/// `content` stays as it is.
pub fn toggle(content: &str, index: usize, today: &str) -> String {
    let mut headings = 0;
    content
        .split_inclusive('\n')
        .map(|line| {
            let Some(heading) = line.strip_prefix("## ") else {
                return line.to_string();
            };
            headings += 1;
            if headings != index + 1 {
                return line.to_string();
            }
            let text = heading.trim_end_matches(['\n', '\r']);
            let ending = &heading[text.len()..];
            let (title, done) = split_marker(text);
            if done == Some(today) {
                format!("## {title}{ending}")
            } else {
                format!("## {title}{MARKER_START}{today}]{ending}")
            }
        })
        .collect()
}

/// `Title [done 2026-10-17]` -> `("Title", Some("2026-10-17"))`.
fn split_marker(heading: &str) -> (&str, Option<&str>) {
    heading
        .strip_suffix(']')
        .and_then(|rest| rest.rsplit_once(MARKER_START))
        .map_or((heading, None), |(title, date)| (title, Some(date)))
}

#[cfg(test)]
mod tests {
    use super::*;

    const TODAY: &str = "2026-10-17";

    #[test]
    fn parse_headings_bodies_and_markers() {
        let tasks = parse(
            "# Daily\n\nintro\n\n## Book time [done 2026-10-17]\n\n## Menu\n\nhttps://menu\n",
        );
        assert_eq!(
            tasks,
            [
                DailyTask {
                    title: "Book time".to_string(),
                    body: String::new(),
                    done: Some(TODAY.to_string()),
                },
                DailyTask {
                    title: "Menu".to_string(),
                    body: "https://menu".to_string(),
                    done: None,
                },
            ]
        );
        assert!(tasks[0].done_on(TODAY));
        assert!(!tasks[0].done_on("2026-10-18"), "a new day starts open");
    }

    #[test]
    fn toggle_marks_unmarks_and_replaces_old_dates() {
        let open = "# Daily\n\n## Book time\nnotes\n## Menu\r\n";
        let done = toggle(open, 0, TODAY);
        assert_eq!(
            done,
            "# Daily\n\n## Book time [done 2026-10-17]\nnotes\n## Menu\r\n"
        );
        assert_eq!(toggle(&done, 0, TODAY), open, "toggles back");

        // Done yesterday counts as open: today's date replaces it.
        assert_eq!(
            toggle(&done, 0, "2026-10-18"),
            done.replace(TODAY, "2026-10-18")
        );
        assert_eq!(
            toggle(open, 1, TODAY),
            "# Daily\n\n## Book time\nnotes\n## Menu [done 2026-10-17]\r\n",
            "keeps the line ending"
        );
    }
}
