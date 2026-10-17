//! The daily checklist in the list, experimental like [`crate::daily`]: its header and its
//! tasks, on top of the sections.

use ratatui::{
    style::{Color, Style, Stylize},
    text::Span,
};

use super::rule;
use crate::{
    app::{self, App},
    daily::DailyTask,
};

/// Outside the section colors, so those stay as they are.
pub const COLOR: Color = Color::Cyan;

/// The tasks of the checklist, none if it is not set.
fn tasks(app: &App) -> &[DailyTask] {
    app.daily.as_deref().unwrap_or_default()
}

/// `── daily ──── 1/2`: how many are still open today of all.
pub fn header(app: &App, width: usize) -> Vec<Span<'static>> {
    let today = app::today();
    let open = tasks(app)
        .iter()
        .filter(|task| !task.done_on(&today))
        .count();
    let count = format!("{open}/{}", tasks(app).len());
    rule(None, "daily", vec![count.dim()], COLOR, width)
}

/// Task `i`, crossed out once done today.
pub fn task(app: &App, i: usize, style: Style) -> Vec<Span<'static>> {
    let task = &tasks(app)[i];
    let style = match task.done_on(&app::today()) {
        true => style.dim().crossed_out(),
        false => style,
    };
    vec![Span::styled(task.title.clone(), style)]
}

/// The body of task `i`, for the preview.
pub fn body(app: &App, i: usize) -> &str {
    &tasks(app)[i].body
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use ratatui::{Terminal, backend::TestBackend, style::Modifier};

    use crate::{
        app::{self, App, tests::sections},
        ui::{render, tests::screen},
    };

    #[test]
    fn daily_shows_open_count_and_strikes_done() {
        let mut app = App::new(PathBuf::new(), sections(&[("home", &[])]));
        let today = app::today();
        app.daily = Some(crate::daily::parse(&format!(
            "## Jira [done {today}]\n## Menu [done 2000-01-01]\n\nhttps://menu\n"
        )));
        let mut terminal = Terminal::new(TestBackend::new(50, 9)).unwrap();
        terminal
            .draw(|frame| {
                render(&app, frame);
            })
            .unwrap();
        let buffer = terminal.backend().buffer();
        let lines: Vec<String> = screen(&app).lines().map(str::to_string).collect();

        // Right below the header, above the sections.
        assert!(
            lines[2].contains("daily") && lines[2].trim_end().ends_with(" 1/2"),
            "{lines:#?}"
        );
        assert!(lines[3].contains("• Jira"), "{lines:#?}");
        assert!(
            buffer[(7, 3)]
                .modifier
                .contains(Modifier::CROSSED_OUT | Modifier::DIM)
        );
        // An old date is open again.
        assert!(lines[4].contains("• Menu"), "{lines:#?}");
        assert!(!buffer[(7, 4)].modifier.contains(Modifier::CROSSED_OUT));
        assert!(lines[6].contains("home"), "{lines:#?}");

        // The body shows in the preview, like for tasks.
        app.cursor = 2;
        assert!(screen(&app).contains("https://menu"));
    }
}
