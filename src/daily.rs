//! The daily checklist ([`FILE`] in the root, experimental): a checklist that starts fresh every
//! day. An add-on kept apart from the tasks, until
//! recurring tasks get a real design. Pure string handling.
//!
//! Each `## ` heading is a task, the lines up to the next one its body. Text before the
//! first heading (a `# Daily` title) is ignored. Done marks the heading with today's date:
//! `## Book time in Jira [done 2026-10-17]`. An older date counts as open.

/// The checklist file in the root, there is no checklist without it.
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

/// `D`: removes task `index` with its body.
pub fn delete(content: &str, index: usize) -> String {
    let (preamble, mut tasks) = split(content);
    let (_, gap) = tasks.remove(index);
    // The last task leaves its gap, so the end of the file stays as it was.
    if index == tasks.len()
        && let Some(last) = tasks.last_mut()
    {
        last.1 = gap;
    }
    join(&preamble, tasks)
}

/// `J` / `K`: swaps task `index` with its neighbour, `None` at the edges.
pub fn move_task(content: &str, index: usize, down: bool) -> Option<String> {
    let (preamble, mut tasks) = split(content);
    let other = if down {
        index + 1
    } else {
        index.checked_sub(1)?
    };
    if other >= tasks.len() {
        return None;
    }
    // Only the tasks swap, the blank lines between them stay where they are.
    let task = std::mem::take(&mut tasks[index].0);
    tasks[index].0 = std::mem::replace(&mut tasks[other].0, task);
    Some(join(&preamble, tasks))
}

/// `o` / `O`: a new task `## title` with `body` at `index`, apart from its neighbours like
/// the other tasks are.
pub fn insert(content: &str, index: usize, title: &str, body: &str) -> String {
    let (mut preamble, mut tasks) = split(content);
    let mut task = format!("## {title}\n");
    if !body.trim().is_empty() {
        task.push_str(&format!("\n{}\n", body.trim_end()));
    }
    let gap = (tasks.iter())
        .map(|(_, gap)| gap.clone())
        .find(|gap| !gap.is_empty())
        .unwrap_or_else(|| "\n".to_string());
    let count = tasks.len();
    match tasks.last_mut() {
        // At the end it takes over the end of the file.
        Some(last) if index == count => {
            let end = std::mem::replace(&mut last.1, gap);
            tasks.push((task, end));
        }
        Some(_) => tasks.insert(index, (task, gap)),
        None => {
            if !preamble.is_empty() && !preamble.ends_with("\n\n") {
                preamble.push_str(if preamble.ends_with('\n') {
                    "\n"
                } else {
                    "\n\n"
                });
            }
            tasks.push((task, String::new()));
        }
    }
    join(&preamble, tasks)
}

/// The text before the first task, then each task as its lines and the blank lines after it.
fn split(content: &str) -> (String, Vec<(String, String)>) {
    let mut preamble = String::new();
    let mut tasks: Vec<(String, String)> = Vec::new();
    for line in content.split_inclusive('\n') {
        if line.starts_with("## ") {
            tasks.push((line.to_string(), String::new()));
            continue;
        }
        match tasks.last_mut() {
            Some((_, gap)) if line.trim().is_empty() => gap.push_str(line),
            Some((task, gap)) => {
                task.push_str(&std::mem::take(gap));
                task.push_str(line);
            }
            None => preamble.push_str(line),
        }
    }
    (preamble, tasks)
}

/// Back to text. A task without a final newline only stays so at the very end.
fn join(preamble: &str, tasks: Vec<(String, String)>) -> String {
    let count = tasks.len();
    let mut content = preamble.to_string();
    for (i, (task, gap)) in tasks.into_iter().enumerate() {
        content.push_str(&task);
        if !task.ends_with('\n') && (i + 1 < count || !gap.is_empty()) {
            content.push('\n');
        }
        content.push_str(&gap);
    }
    content
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

    const FILE: &str = "# Daily\n\n## A\nnotes\n\nmore\n\n## B\n\n## C\n";

    #[test]
    fn delete_keeps_the_rest_as_it_is() {
        assert_eq!(delete(FILE, 0), "# Daily\n\n## B\n\n## C\n");
        assert_eq!(delete(FILE, 1), "# Daily\n\n## A\nnotes\n\nmore\n\n## C\n");
        assert_eq!(delete(FILE, 2), "# Daily\n\n## A\nnotes\n\nmore\n\n## B\n");
        assert_eq!(delete("# Daily\n\n## A\n", 0), "# Daily\n\n");
    }

    #[test]
    fn move_swaps_tasks_not_the_gaps() {
        assert_eq!(
            move_task(FILE, 0, true).unwrap(),
            "# Daily\n\n## B\n\n## A\nnotes\n\nmore\n\n## C\n"
        );
        assert_eq!(
            move_task(FILE, 2, false).unwrap(),
            "# Daily\n\n## A\nnotes\n\nmore\n\n## C\n\n## B\n"
        );
        assert_eq!(move_task(FILE, 0, false), None);
        assert_eq!(move_task(FILE, 2, true), None);
        // Without a final newline, the task that ends up last keeps it, the other gets one.
        assert_eq!(move_task("## A\n## B", 0, true).unwrap(), "## B\n## A\n");
    }

    #[test]
    fn insert_apart_like_the_others() {
        assert_eq!(
            insert(FILE, 1, "New", "body\n"),
            "# Daily\n\n## A\nnotes\n\nmore\n\n## New\n\nbody\n\n## B\n\n## C\n"
        );
        assert_eq!(insert(FILE, 3, "New", ""), format!("{FILE}\n## New\n"));
        assert_eq!(
            insert(FILE, 0, "New", ""),
            FILE.replace("## A", "## New\n\n## A")
        );
        assert_eq!(insert("# Daily\n", 0, "New", ""), "# Daily\n\n## New\n");
        assert_eq!(insert("", 0, "New", ""), "## New\n");
        assert_eq!(insert("## A", 1, "New", ""), "## A\n\n## New\n");
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
