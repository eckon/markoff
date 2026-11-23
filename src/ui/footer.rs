//! The footer: the filter input or the status on the left, key hints on the right.

use ratatui::{
    Frame,
    layout::Rect,
    style::Stylize,
    text::{Line, Span},
};

use super::{LINE_COLOR, help};
use crate::{
    app::{App, Page, Row, Status, archive::Focus},
    keymap::{Action, BINDINGS, Context},
};

/// `── what just happened ──── hints` over the whole width, a line like the app header: left the
/// filter input or the status, right the key hints. Without either side the line runs
/// to the edge.
pub fn render(app: &App, frame: &mut Frame, area: Rect) {
    let left = match app.filter_input {
        true => vec![format!("/{}", app.active_filter()).into()],
        false => status_text(app),
    };
    let mut spans = Vec::new();
    if !left.is_empty() {
        spans.push("── ".fg(LINE_COLOR));
        spans.extend(left);
        spans.push(" ".into());
    }
    let used: usize = spans.iter().map(Span::width).sum();
    if app.filter_input {
        let end = area.x + used.saturating_sub(1) as u16;
        frame.set_cursor_position((end.min(area.right().saturating_sub(1)), area.y));
    }

    // Between the hints, and at least that much line between the two sides.
    const GAP: usize = 2;
    let hint_spans = |hints: &[(String, &str)]| {
        let mut spans: Vec<Span> = Vec::new();
        for (i, (key, text)) in hints.iter().enumerate() {
            spans.push(if i == 0 { " " } else { "  " }.into());
            spans.push(key.clone().bold());
            spans.push(format!(" {text}").dim());
        }
        spans
    };
    let width = |spans: &[Span]| spans.iter().map(Span::width).sum::<usize>();
    // Hints give way from the end, the left side is what just happened.
    let mut hints = hints(app);
    while !hints.is_empty() && used + GAP + width(&hint_spans(&hints)) > area.width as usize {
        hints.pop();
    }
    let hints = hint_spans(&hints);

    let fill = (area.width as usize).saturating_sub(used + width(&hints));
    spans.push("─".repeat(fill).fg(LINE_COLOR));
    spans.extend(hints);
    frame.render_widget(Line::from(spans), area);
}

/// Key hints per mode: how to get out of the filter, and where to find the rest. Keys from
/// [`BINDINGS`], so they always match. The filter input's with their texts, the others with own
/// ones that fit the footer.
fn hints(app: &App) -> Vec<(String, &'static str)> {
    let binding = |context, action| {
        BINDINGS
            .iter()
            .find(|b| b.contexts.contains(&context) && b.keys.iter().any(|(_, a)| *a == action))
    };
    if app.filter_input {
        return [Action::FilterAccept, Action::FilterClear]
            .into_iter()
            .filter_map(|action| binding(Context::Filter, action))
            .map(|b| (b.labels().join(" / "), b.help))
            .collect();
    }
    // The page's keys, also while the help is open.
    let key = |action| binding(Context::Page(app.page), action).map(|b| b.labels().join(" / "));
    let mut hints = Vec::new();
    // The filter is cleared from its input: its key, then `Esc`.
    if !app.active_filter().is_empty() {
        hints.extend(key(Action::Filter).map(|key| (format!("{key} Esc"), "clear filter")));
    }
    // A range is ended with the key that started it or `Esc`, in the heatmap.
    if app.page == Page::Archive
        && app.archive_page.anchor.is_some()
        && app.archive_page.focus == Focus::Heatmap
        && app.help.is_none()
    {
        hints.extend(key(Action::Visual).map(|key| (format!("{key} / Esc"), "end range")));
    }
    hints.extend(key(Action::Help).map(|key| (key, "toggle help")));
    hints
}

/// The status, left in the footer when the filter input is closed, empty for nothing.
fn status_text(app: &App) -> Vec<Span<'_>> {
    match (&app.status, app.keymap.pending()) {
        (Some(Status::Error(message)), _) => vec![message.as_str().red().bold()],
        (Some(Status::Info(message)), _) => vec![message.as_str().into()],
        (None, Some(key)) => vec![key.to_string().into()],
        (None, None)
            if app
                .help
                .as_ref()
                .is_some_and(|help| !help.filter.is_empty()) =>
        {
            let filter = app.active_filter();
            let count = help::bindings(app.page, filter).len();
            let keys = if count == 1 { "key" } else { "keys" };
            vec![
                "filter: ".dim(),
                filter.into(),
                format!(" · {count} {keys}").dim(),
            ]
        }
        (None, None) if !app.page_filter().is_empty() => {
            // The archive counts its matches over all days, not only the selected ones.
            let count = match app.page {
                Page::Archive => app.archive_matches().map_or(0, |m| m.values().sum()),
                Page::Tasks => (app.rows().iter())
                    .filter(|row| matches!(row, Row::Task(..)))
                    .count(),
            };
            let tasks = if count == 1 { "task" } else { "tasks" };
            vec![
                "filter: ".dim(),
                app.page_filter().into(),
                format!(" · {count} {tasks}").dim(),
            ]
        }
        (None, None) => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::{
        app::{App, Status, tests::sections},
        keymap::Action,
        ui::tests::screen,
    };

    #[test]
    fn status_line_shows_errors() {
        let mut app = App::new(
            PathBuf::new(),
            sections(&[("home", &[])]),
            crate::config::defaults(),
        );
        app.update(Action::Undo);
        let screen = screen(&app);
        assert!(
            screen.contains("nothing to undo"),
            "missing status in\n{screen}"
        );
    }

    #[test]
    fn filter_input_and_filtered_list() {
        let mut app = App::new(
            PathBuf::new(),
            sections(&[("home", &["Fix typo"]), ("work", &["Review"])]),
            crate::config::defaults(),
        );
        app.update(Action::Filter);
        for c in "rev".chars() {
            app.update(Action::FilterType(c));
        }
        let lines: Vec<String> = screen(&app).lines().map(str::to_string).collect();
        // The first visible section starts right below the app header, without another blank line.
        assert!(lines[2].contains("work"), "{lines:#?}");
        assert!(lines[3].contains("Review"), "{lines:#?}");
        assert!(!lines.join("\n").contains("Fix typo"));
        assert!(lines[8].starts_with("── /rev ─"), "input at the bottom");
        assert!(lines[8].ends_with("<CR> keep filter  Esc clear filter"));

        // Accepted, the filter stays visible in the footer, with its matches.
        app.update(Action::FilterAccept);
        let screen = screen(&app);
        let status = screen.lines().last().unwrap();
        assert!(
            status.starts_with("── filter: rev · 1 task ─"),
            "{status:?}"
        );
        // `? toggle help` gives way, 50 columns are too narrow for both hints.
        assert!(status.ends_with("Esc clear filter"), "{status:?}");
    }

    #[test]
    fn status_hints_give_way_to_messages() {
        let mut app = App::new(
            PathBuf::new(),
            sections(&[("home", &[])]),
            crate::config::defaults(),
        );
        assert!(
            screen(&app)
                .lines()
                .last()
                .unwrap()
                .ends_with("? toggle help")
        );

        // 50 columns: no room for the hint next to this message.
        let message = "x".repeat(44);
        app.status = Some(Status::Info(message.clone()));
        assert_eq!(
            screen(&app).lines().last().unwrap(),
            format!("── {message} ──")
        );
    }
}
