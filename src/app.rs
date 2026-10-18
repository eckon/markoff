//! App state, the run loop, and what each [`Action`] does to it and to the disk. What needs
//! `$EDITOR` is in [`editor`], the experimental daily checklist in [`daily`].

use std::{
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

use color_eyre::{
    Result,
    eyre::{WrapErr, eyre},
};
use ratatui::{
    DefaultTerminal,
    crossterm::event::{self, Event, KeyCode, KeyEventKind},
};

mod daily;
mod editor;

use editor::EditorRequest;

use crate::{
    archive,
    daily::DailyTask,
    edit, filter, history,
    keymap::{self, Action, Keymap},
    load,
    op::{self, Op},
    order::{self, OrderFile},
    task::{self, Section},
    ui,
    watch::TodoWatcher,
};

/// How long to wait for a key before checking the file watcher again.
const POLL_INTERVAL: Duration = Duration::from_millis(50);

/// One line in the list. The cursor moves over these.
///
/// Section headers are rows too, so the cursor can land on an empty section (to add a task or move a section).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Row {
    Section(usize),
    /// Section index, task index within that section.
    Task(usize, usize),
    /// The `daily` header, on top of the sections, see [`daily`] (experimental).
    Daily,
    /// Task index in `daily.md`.
    DailyTask(usize),
}

/// A screen of its own, switched with `[` / `]` in this order. The help is an overlay on top.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    Todo,
    /// The done tasks: heatmap and stats, see [`crate::ui`].
    Archive,
}

impl Page {
    pub const ALL: [Page; 2] = [Page::Todo, Page::Archive];

    pub fn name(self) -> &'static str {
        match self {
            Page::Todo => "todo",
            Page::Archive => "archive",
        }
    }
}

/// Message for the status line, cleared on the next key press.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    Error(String),
    Info(String),
}

/// One user action on the undo stack.
#[derive(Debug)]
struct UndoEntry {
    ops: Vec<Op>,
    /// Task file or section directory the cursor was on before the action. Undo puts it back
    /// there: the undo restores the old names, so the path exists again.
    cursor: Option<PathBuf>,
}

/// Application state.
#[derive(Debug)]
pub struct App {
    running: bool,
    /// Directory with the sections, `.order` and `.archive/`.
    root: PathBuf,
    pub sections: Vec<Section>,
    /// `.order` as read with `sections`, the base for the order an action writes.
    order_file: OrderFile,
    /// Index into [`App::rows`].
    pub cursor: usize,
    pub keymap: Keymap,
    /// Applied actions, newest last. Undo applies the inverse of the last one.
    undo: Vec<UndoEntry>,
    pub status: Option<Status>,
    /// Help overlay is open.
    pub show_help: bool,
    pub page: Page,
    editor_request: Option<EditorRequest>,
    /// `/` filter, empty for none. Applied in [`App::rows`], so everything that works on rows
    /// sees only what matches.
    pub filter: String,
    /// The filter input at the bottom is open and takes the keys.
    pub filter_input: bool,
    /// First list line on screen. Kept between frames so the view only scrolls when the
    /// cursor would leave it, see [`ui::render`].
    pub list_offset: usize,
    /// The daily checklist file from the config, `None` if not set.
    daily_path: Option<PathBuf>,
    /// The tasks of the daily checklist, `None` if it is not set or the file does not exist.
    pub daily: Option<Vec<DailyTask>>,
    /// Tasks done per day, from the archive.
    pub days: history::Days,
    /// The same per section.
    pub section_days: history::Sections,
}

impl App {
    /// Without an order file, [`App::load`] reads the real one.
    pub fn new(root: PathBuf, sections: Vec<Section>) -> Self {
        let order_file = OrderFile {
            path: root.join(order::FILE),
            content: None,
        };
        Self {
            running: true,
            root,
            sections,
            order_file,
            cursor: 0,
            keymap: Keymap::default(),
            undo: Vec::new(),
            status: None,
            show_help: false,
            page: Page::Todo,
            editor_request: None,
            filter: String::new(),
            filter_input: false,
            list_offset: 0,
            daily_path: None,
            daily: None,
            days: history::Days::new(),
            section_days: history::Sections::new(),
        }
    }

    /// Loads `root` and normalizes it, renames are mentioned in the status line. `daily` is
    /// the daily checklist file, if set.
    pub fn load(root: PathBuf, daily: Option<PathBuf>) -> Result<Self> {
        let mut app = Self::new(root, Vec::new());
        app.daily_path = daily;
        app.reload(None)?;
        app.normalize_before_action();
        Ok(app)
    }

    /// Main loop: draw if needed, wait briefly for a key, check for outside changes.
    ///
    /// Keys are read on this thread (no background reader), so the TUI can hand the
    /// terminal to `$EDITOR` without the two fighting over stdin.
    pub fn run(mut self, terminal: &mut DefaultTerminal) -> Result<()> {
        let mut watcher = TodoWatcher::new(&self.root)?;
        // Only redraw after something happened, not on every poll.
        let mut dirty = true;

        while self.running {
            if dirty {
                let mut offset = self.list_offset;
                terminal.draw(|frame| offset = ui::render(&self, frame))?;
                self.list_offset = offset;
                dirty = false;
            }

            // Any terminal event counts, a resize needs a redraw too.
            if event::poll(POLL_INTERVAL)? {
                self.handle_event(event::read()?);
                dirty = true;
            }

            // Our own writes trigger this as well, a reload is harmless then.
            match watcher.poll() {
                Ok(false) => {}
                Ok(true) => {
                    let result = self.reload_keep_selection();
                    self.report(result);
                    dirty = true;
                }
                Err(err) => {
                    self.report(Err(err));
                    dirty = true;
                }
            }

            if let Some(request) = self.editor_request.take() {
                let result = self.run_editor(terminal, request);
                self.report(result);
                dirty = true;
            }
        }
        Ok(())
    }

    /// Reloads but keeps the cursor on the same task or section, even if rows above it
    /// appeared or disappeared.
    fn reload_keep_selection(&mut self) -> Result<()> {
        let selected = self.selected().map(|row| self.row_path(row));
        self.reload(selected.as_deref())
    }

    /// Path behind a row: the task file or the section directory. Never touches the disk,
    /// only keeps the cursor on the right row, e.g. after undo.
    fn row_path(&self, row: Row) -> PathBuf {
        match row {
            Row::Section(s) => self.sections[s].dir.clone(),
            Row::Task(s, t) => self.sections[s].tasks[t].path.clone(),
            Row::Daily => self.daily_row_path(None),
            Row::DailyTask(i) => self.daily_row_path(Some(i)),
        }
    }

    fn handle_event(&mut self, event: Event) {
        // Only presses: some terminals also report key releases.
        if let Event::Key(key) = event
            && key.kind == KeyEventKind::Press
        {
            self.status = None;
            let action = if self.filter_input {
                keymap::filter_input(key)
            } else {
                self.keymap.handle(key, self.page)
            };
            // The quit keys quit everywhere, also from the help.
            if action == Some(Action::Quit) {
                self.running = false;
                return;
            }
            // Only `?` and `Esc` close the help, other keys do nothing, so a stray key cannot
            // change tasks that are dimmed behind it. `Esc` by its code, on the archive page it
            // has no binding.
            if self.show_help {
                if action == Some(Action::Help) || key.code == KeyCode::Esc {
                    self.show_help = false;
                }
                return;
            }
            if let Some(action) = action {
                self.update(action);
            }
        }
    }

    /// Runs an action. Errors end up in the status line instead of ending the app.
    pub fn update(&mut self, action: Action) {
        let result = self.try_update(action);
        self.report(result);
    }

    fn report(&mut self, result: Result<()>) {
        if let Err(err) = result {
            self.status = Some(Status::Error(err.to_string()));
        }
    }

    fn try_update(&mut self, action: Action) -> Result<()> {
        // Where a task goes is unclear while tasks in between are hidden, and a new task or
        // section would mostly be hidden right away.
        if !self.filter.is_empty()
            && matches!(
                action,
                Action::MoveDown
                    | Action::MoveUp
                    | Action::NewBelow
                    | Action::NewAbove
                    | Action::NewSection
            )
        {
            self.status = Some(Status::Info("clear the filter first (Esc)".to_string()));
            return Ok(());
        }
        // Writing actions see clean names. Not undo: its inverse fits the files as they are.
        if matches!(
            action,
            Action::MoveDown | Action::MoveUp | Action::Done | Action::Delete
        ) {
            self.normalize_before_action();
        }
        match action {
            Action::Up => self.cursor = self.cursor.saturating_sub(1),
            Action::Down => {
                let last = self.rows().len().saturating_sub(1);
                self.cursor = (self.cursor + 1).min(last);
            }
            Action::Top => self.cursor = 0,
            Action::Bottom => self.cursor = self.rows().len().saturating_sub(1),
            Action::PrevSection => self.cursor = self.prev_section_row(),
            Action::NextSection => self.cursor = self.next_section_row(),
            Action::JumpSection(s) => {
                // Nothing if the section does not exist or the filter hides it.
                let rows = self.rows();
                if let Some(header) = rows.iter().position(|&row| row == Row::Section(s)) {
                    let has_task = matches!(rows.get(header + 1), Some(Row::Task(..)));
                    self.cursor = header + usize::from(has_task);
                }
            }
            Action::MoveDown | Action::MoveUp => self.move_selected(action == Action::MoveDown)?,
            Action::Open => self.open(),
            Action::NewBelow | Action::NewAbove => self.new_task(action == Action::NewBelow),
            Action::NewSection => self.new_section(),
            Action::Done => self.done()?,
            Action::Delete => self.delete()?,
            Action::Undo => self.undo()?,
            Action::Help => self.show_help = true,
            Action::PrevPage | Action::NextPage => {
                let count = Page::ALL.len();
                let step = if action == Action::NextPage {
                    1
                } else {
                    count - 1
                };
                let at = Page::ALL.iter().position(|&page| page == self.page);
                self.page = Page::ALL[(at.unwrap_or(0) + step) % count];
            }
            Action::Quit => self.running = false,
            Action::Hint(hint) => self.status = Some(Status::Info(hint.to_string())),
            Action::Filter => self.filter_input = true,
            Action::FilterType(c) => self.set_filter(|filter| filter.push(c)),
            Action::FilterBackspace => self.set_filter(|filter| {
                filter.pop();
            }),
            // Trailing spaces go with the word before them, so repeating it keeps deleting.
            Action::FilterDeleteWord => self.set_filter(|filter| {
                let kept = filter
                    .trim_end()
                    .trim_end_matches(|c: char| !c.is_whitespace());
                filter.truncate(kept.len());
            }),
            Action::FilterAccept => self.filter_input = false,
            Action::FilterClear => {
                self.filter_input = false;
                self.set_filter(String::clear);
            }
        }
        Ok(())
    }

    /// Changes the filter. The cursor stays on its task or section if that still matches,
    /// otherwise it goes to the first row.
    fn set_filter(&mut self, change: impl FnOnce(&mut String)) {
        let selected = self.selected().map(|row| self.row_path(row));
        change(&mut self.filter);
        self.cursor = 0;
        if let Some(path) = selected {
            self.select_path(&path);
        }
    }

    /// Renames all tasks into shape and cleans the order file with [`edit::normalize`].
    /// Returns the renames.
    ///
    /// Not added to undo: undoing it would only be redone by the next action. The cursor
    /// follows a renamed task.
    fn normalize(&mut self) -> Result<Vec<(PathBuf, PathBuf)>> {
        let ops = edit::normalize(&self.sections, &self.order_file);
        if ops.is_empty() {
            return Ok(Vec::new());
        }
        let renames: Vec<(PathBuf, PathBuf)> = ops
            .iter()
            .filter_map(|op| match op {
                Op::Rename { from, to } => Some((from.clone(), to.clone())),
                _ => None,
            })
            .collect();

        let selected = self.selected().map(|row| {
            let path = self.row_path(row);
            renames
                .iter()
                .find(|(from, _)| *from == path)
                .map_or(path, |(_, to)| to.clone())
        });
        op::apply_all(&ops)?;
        self.reload(selected.as_deref())?;
        Ok(renames)
    }

    /// [`Self::normalize`] at startup and before an action that writes, so the action sees
    /// clean names.
    ///
    /// Renames are mentioned in the status line. A failure is reported there too, everything
    /// was rolled back then, so the action still runs on the unchanged files.
    fn normalize_before_action(&mut self) {
        match self.normalize() {
            Ok(renames) if renames.is_empty() => {}
            Ok(renames) => self.status = Some(Status::Info(renamed_message(renames.len()))),
            Err(err) => self.report(Err(err)),
        }
    }

    fn done(&mut self) -> Result<()> {
        let (s, t) = match self.selected() {
            Some(Row::Task(s, t)) => (s, t),
            Some(Row::DailyTask(i)) => return self.toggle_daily(i),
            _ => return Ok(()),
        };
        let date = today();
        let archive_dir = self.root.join(archive::DIR);
        let archive_path = archive_dir.join(&date).with_extension(task::EXTENSION);
        // Not an op: an empty `.archive/` left behind by undo does no harm.
        fs::create_dir_all(&archive_dir)
            .wrap_err_with(|| format!("failed to create {}", archive_dir.display()))?;
        let existing = load::read_if_exists(&archive_path)?;

        let ops = edit::archive(
            &self.sections,
            s,
            t,
            &archive_path,
            existing.as_deref(),
            &date,
            &self.order_file,
        );
        self.apply(ops)?;
        self.status = Some(Status::Info("task archived (u to undo)".to_string()));
        Ok(())
    }

    /// `J` / `K`: moves the task or section under the cursor one place, the cursor follows.
    /// Nothing happens at the edges.
    fn move_selected(&mut self, down: bool) -> Result<()> {
        let file = &self.order_file;
        let (ops, moved_to) = match self.selected() {
            Some(Row::Task(s, t)) => {
                let Some((to, index)) = self.task_target(s, t, down) else {
                    return Ok(());
                };
                edit::move_task(&self.sections, s, t, to, index, file)
            }
            Some(Row::Section(s)) => match edit::move_section(&self.sections, s, down, file) {
                Some(result) => result,
                None => return Ok(()),
            },
            Some(Row::Daily | Row::DailyTask(_)) | None => return Ok(()),
        };
        self.apply(ops)?;
        self.select_path(&moved_to);
        Ok(())
    }

    /// `{`: the nearest section header above the cursor. On a task that is its own section.
    fn prev_section_row(&self) -> usize {
        let rows = self.rows();
        (0..self.cursor)
            .rev()
            .find(|&i| matches!(rows[i], Row::Section(_) | Row::Daily))
            .unwrap_or(0)
    }

    /// `}`: the next section header below the cursor, the cursor stays if there is none.
    fn next_section_row(&self) -> usize {
        let rows = self.rows();
        (self.cursor + 1..rows.len())
            .find(|&i| matches!(rows[i], Row::Section(_) | Row::Daily))
            .unwrap_or(self.cursor)
    }

    /// Where `J` / `K` put task `t` of section `s`, as section and index among the other
    /// tasks there. At a section edge it goes into the neighbouring section, first or last.
    /// `None` at the very top or bottom.
    fn task_target(&self, s: usize, t: usize, down: bool) -> Option<(usize, usize)> {
        if down {
            if t + 1 < self.sections[s].tasks.len() {
                Some((s, t + 1))
            } else {
                (s + 1 < self.sections.len()).then_some((s + 1, 0))
            }
        } else if t > 0 {
            Some((s, t - 1))
        } else {
            let previous = s.checked_sub(1)?;
            Some((previous, self.sections[previous].tasks.len()))
        }
    }

    /// `D`: deletes the task without archiving. Undo brings it back.
    fn delete(&mut self) -> Result<()> {
        let (s, t) = match self.selected() {
            Some(Row::Task(s, t)) => (s, t),
            Some(Row::Section(s)) => return self.delete_section(s),
            Some(Row::Daily | Row::DailyTask(_)) | None => return Ok(()),
        };
        // Cursor stays on the same row, which is now the next task (like vim).
        let ops = edit::delete(&self.sections, s, t, &self.order_file);
        self.apply(ops)?;
        self.status = Some(Status::Info("task deleted (u to undo)".to_string()));
        Ok(())
    }

    /// Only empty sections, so `D` never takes tasks with it.
    fn delete_section(&mut self, s: usize) -> Result<()> {
        let section = &self.sections[s];
        let title = section.name.clone();
        if !section.tasks.is_empty() {
            return Err(eyre!("{title} is not empty"));
        }
        // Cursor stays on the same row, which is now the next section.
        let ops = edit::delete_section(&self.sections, s, &self.order_file);
        self.apply(ops)?;
        self.status = Some(Status::Info("section deleted (u to undo)".to_string()));
        Ok(())
    }

    fn undo(&mut self) -> Result<()> {
        let Some(entry) = self.undo.last() else {
            return Err(eyre!("nothing to undo"));
        };
        // Only pop once the inverse worked, so a failed undo can be retried.
        op::apply_all(&op::inverse_all(&entry.ops))?;
        let cursor = entry.cursor.clone();
        self.undo.pop();
        self.reload(cursor.as_deref())
    }

    /// Applies ops, records them for undo together with the cursor position, and reloads.
    fn apply(&mut self, ops: Vec<Op>) -> Result<()> {
        let cursor = self.selected().map(|row| self.row_path(row));
        op::apply_all(&ops)?;
        self.undo.push(UndoEntry { ops, cursor });
        self.reload(None)
    }

    /// Reads the root again and puts the cursor on `select` if it still exists, else keeps the
    /// row number, clamped. The disk is the source of truth, `sections` is only a view of it.
    fn reload(&mut self, select: Option<&Path>) -> Result<()> {
        (self.sections, self.order_file) = load::load(&self.root)?;
        self.reload_daily()?;
        (self.days, self.section_days) = history::split(&load::archive(&self.root)?);
        self.cursor = self.cursor.min(self.rows().len().saturating_sub(1));
        if let Some(path) = select {
            self.select_path(path);
        }
        Ok(())
    }

    /// Moves the cursor to the task file or section directory at `path`, if it exists.
    fn select_path(&mut self, path: &Path) {
        let found = self
            .rows()
            .into_iter()
            .position(|row| self.row_path(row) == path);
        if let Some(index) = found {
            self.cursor = index;
        }
    }

    /// All rows in display order: the daily checklist, then each section header with its tasks.
    /// A filter keeps matching tasks under their header and hides the rest, daily included.
    /// Computed on demand, so it never gets out of sync with `sections`.
    pub fn rows(&self) -> Vec<Row> {
        let mut rows = self.daily_rows();
        for (s, section) in self.sections.iter().enumerate() {
            let tasks: Vec<Row> = (0..section.tasks.len())
                .filter(|&t| {
                    let task = &section.tasks[t];
                    filter::matches(&task.title(), &self.filter)
                        || filter::matches(&task.body, &self.filter)
                })
                .map(|t| Row::Task(s, t))
                .collect();
            if self.filter.is_empty() || !tasks.is_empty() {
                rows.push(Row::Section(s));
                rows.extend(tasks);
            }
        }
        rows
    }

    /// The row under the cursor, `None` only if there are no rows at all.
    pub fn selected(&self) -> Option<Row> {
        self.rows().get(self.cursor).copied()
    }

    /// Tasks archived today, shown in the header.
    pub fn done_today(&self) -> usize {
        let today = jiff::Zoned::now().date();
        self.days.get(&today).copied().unwrap_or(0)
    }
}

/// `YYYY-MM-DD`, for the archive file and the daily marker.
pub fn today() -> String {
    jiff::Zoned::now().date().to_string()
}

fn renamed_message(count: usize) -> String {
    if count == 1 {
        "1 file renamed to match its title".to_string()
    } else {
        format!("{count} files renamed to match their titles")
    }
}

#[cfg(test)]
pub mod tests {
    use std::fs;

    use ratatui::crossterm::event::{KeyEvent, KeyModifiers};
    use tempfile::TempDir;

    use super::*;
    use crate::{
        order::{Group, Order},
        task::Task,
    };

    /// Sections with the given names and task titles. Nothing on disk.
    pub fn sections(spec: &[(&str, &[&str])]) -> Vec<Section> {
        spec.iter()
            .map(|(name, tasks)| Section {
                name: name.to_string(),
                dir: PathBuf::new(),
                tasks: tasks
                    .iter()
                    .map(|title| Task::parse(PathBuf::new(), &format!("# {title}")))
                    .collect(),
            })
            .collect()
    }

    /// An app on a temp root with the given files, each in a section: `work/a.md`. Paths
    /// ending in `/` become empty sections. Each file gets its name as heading: `a.md` ->
    /// `# a`. The order file lists everything in the given order.
    pub(super) fn app_with_files(files: &[&str]) -> (TempDir, App) {
        let dir = TempDir::new().unwrap();
        let root = dir.path();
        let mut order = Order::default();
        for file in files {
            let (section, name) = file.split_once('/').expect("tasks live in a section");
            if order.section_position(section).is_none() {
                order.sections.push(Group {
                    name: section.to_string(),
                    tasks: Vec::new(),
                });
            }
            let path = root.join(file);
            if name.is_empty() {
                fs::create_dir_all(path).unwrap();
                continue;
            }
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            let stem = name.trim_end_matches(".md");
            fs::write(&path, format!("# {stem}")).unwrap();
            let group = order
                .sections
                .iter_mut()
                .find(|group| group.name == section);
            group.unwrap().tasks.push(stem.to_string());
        }
        fs::write(root.join(order::FILE), order.format()).unwrap();
        let app = App::load(root.to_path_buf(), None).unwrap();
        (dir, app)
    }

    pub(super) fn order_file(dir: &TempDir) -> String {
        fs::read_to_string(dir.path().join(order::FILE)).unwrap()
    }

    /// File names on disk in row order, sections as `[name]`.
    pub(super) fn listing(app: &App) -> Vec<String> {
        app.rows()
            .into_iter()
            .map(|row| match row {
                Row::Section(s) => format!("[{}]", app.sections[s].name),
                Row::Daily => "[daily]".to_string(),
                Row::DailyTask(i) => app.daily.as_ref().unwrap()[i].title.clone(),
                Row::Task(s, t) => {
                    let path = &app.sections[s].tasks[t].path;
                    path.file_name().unwrap().to_str().unwrap().to_string()
                }
            })
            .collect()
    }

    #[test]
    fn rows_include_section_headers() {
        let app = App::new(
            PathBuf::new(),
            sections(&[("work", &["a"]), ("home", &[]), ("x", &["b", "c"])]),
        );
        assert_eq!(
            app.rows(),
            [
                Row::Section(0),
                Row::Task(0, 0),
                Row::Section(1),
                Row::Section(2),
                Row::Task(2, 0),
                Row::Task(2, 1),
            ]
        );
    }

    #[test]
    fn cursor_stays_in_bounds() {
        let mut app = App::new(PathBuf::new(), sections(&[("work", &["a"])]));
        app.update(Action::Up);
        assert_eq!(app.cursor, 0);
        app.update(Action::Down);
        app.update(Action::Down);
        assert_eq!(app.cursor, 1);
    }

    #[test]
    fn only_help_and_esc_close_help() {
        let (_dir, mut app) = app_with_files(&["work/a.md"]);
        app.cursor = 1;
        app.update(Action::Help);
        assert!(app.show_help);

        app.handle_event(Event::Key(KeyCode::Char('X').into()));
        assert!(app.show_help, "other keys keep the help open");
        assert_eq!(listing(&app), ["[work]", "a.md"], "X did not archive");

        for code in [KeyCode::Char('?'), KeyCode::Esc] {
            app.update(Action::Help);
            app.handle_event(Event::Key(code.into()));
            assert!(!app.show_help, "{code:?} closes the help");
            assert!(app.running, "{code:?} does not quit");
        }
    }

    #[test]
    fn pages_wrap_around_and_keep_todo_keys_to_the_list() {
        let (_dir, mut app) = app_with_files(&["work/a.md"]);
        app.cursor = 1;
        app.handle_event(Event::Key(KeyCode::Char(']').into()));
        assert_eq!(app.page, Page::Archive);

        app.handle_event(Event::Key(KeyCode::Char('X').into()));
        assert_eq!(listing(&app), ["[work]", "a.md"], "X did not archive");

        app.handle_event(Event::Key(KeyCode::Char(']').into()));
        assert_eq!(app.page, Page::Todo, "wraps around");
        app.handle_event(Event::Key(KeyCode::Char('[').into()));
        assert_eq!(app.page, Page::Archive);
    }

    #[test]
    fn quit_keys_quit_from_help() {
        let ctrl_c = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
        for key in [KeyEvent::from(KeyCode::Char('q')), ctrl_c] {
            let (_dir, mut app) = app_with_files(&["work/a.md"]);
            app.update(Action::Help);
            app.handle_event(Event::Key(key));
            assert!(!app.running, "{key:?} quits from the help");
        }
    }

    #[test]
    fn root_files_are_no_tasks() {
        let (dir, mut app) = app_with_files(&["work/a.md"]);
        fs::write(dir.path().join("notes.md"), "# Notes").unwrap();
        app.reload(None).unwrap();
        app.update(Action::MoveDown);

        assert_eq!(listing(&app), ["[work]", "a.md"]);
        assert!(dir.path().join("notes.md").exists(), "never renamed");
        assert_eq!(order_file(&dir), "# work\na\n");
    }

    #[test]
    fn reload_keeps_cursor_on_the_same_task() {
        let (dir, mut app) = app_with_files(&["work/a.md", "work/b.md"]);
        app.cursor = 2;

        // Changes from outside (a git pull): tasks appear above, one disappears.
        let work = dir.path().join("work");
        fs::write(work.join("z.md"), "# z").unwrap();
        fs::write(work.join("y.md"), "# y").unwrap();
        fs::remove_file(work.join("a.md")).unwrap();
        fs::write(dir.path().join(order::FILE), "# work\ny\nz\nb\n").unwrap();
        app.reload_keep_selection().unwrap();

        assert_eq!(listing(&app), ["[work]", "y.md", "z.md", "b.md"]);
        assert_eq!(app.cursor, 3, "still on b.md");
    }

    #[test]
    fn reload_clamps_cursor_if_task_is_gone() {
        let (dir, mut app) = app_with_files(&["work/a.md"]);
        app.cursor = 1;
        fs::remove_file(dir.path().join("work/a.md")).unwrap();
        app.reload_keep_selection().unwrap();
        assert_eq!(app.cursor, 0);
    }

    #[test]
    fn quit_stops_running() {
        let mut app = App::new(PathBuf::new(), sections(&[]));
        app.update(Action::Quit);
        assert!(!app.running);
    }

    #[test]
    fn delete_and_undo() {
        let (dir, mut app) = app_with_files(&["work/a.md", "work/b.md"]);
        app.cursor = 1;

        app.update(Action::Delete);
        assert_eq!(listing(&app), ["[work]", "b.md"]);
        assert_eq!(order_file(&dir), "# work\nb\n");
        assert_eq!(app.cursor, 1, "cursor moves onto the next task");
        assert_eq!(
            app.status,
            Some(Status::Info("task deleted (u to undo)".to_string()))
        );

        app.update(Action::Undo);
        assert_eq!(listing(&app), ["[work]", "a.md", "b.md"]);
        assert_eq!(order_file(&dir), "# work\na\nb\n");
        assert_eq!(app.cursor, 1, "cursor is on the restored task");

        app.update(Action::Undo);
        assert_eq!(
            app.status,
            Some(Status::Error("nothing to undo".to_string()))
        );
    }

    #[test]
    fn delete_section_only_if_empty() {
        let (dir, mut app) = app_with_files(&["work/a.md", "home/", "x/b.md"]);
        let before = listing(&app);

        app.update(Action::Delete);
        assert_eq!(
            app.status,
            Some(Status::Error("work is not empty".to_string()))
        );
        assert_eq!(listing(&app), before);

        // The empty `home` goes, `x` moves up into its place, under the cursor.
        app.cursor = 2;
        app.update(Action::Delete);
        assert_eq!(listing(&app), ["[work]", "a.md", "[x]", "b.md"]);
        assert!(!dir.path().join("home").exists());
        assert_eq!(order_file(&dir), "# work\na\n\n# x\nb\n");
        assert_eq!(app.selected(), Some(Row::Section(1)));
        assert_eq!(
            app.status,
            Some(Status::Info("section deleted (u to undo)".to_string()))
        );

        app.update(Action::Undo);
        assert_eq!(listing(&app), before);
        assert_eq!(app.cursor, 2, "back on `home`");
    }

    /// Types `text` into the filter input, like `/` followed by the keys.
    pub(super) fn type_filter(app: &mut App, text: &str) {
        app.update(Action::Filter);
        for c in text.chars() {
            app.update(Action::FilterType(c));
        }
    }

    #[test]
    fn filter_shows_matching_tasks_under_their_section() {
        let (dir, mut app) = app_with_files(&[
            "work/fix-login.md",
            "work/buy-milk.md",
            "x/review.md",
            "home/garden.md",
        ]);
        fs::write(
            dir.path().join("home/garden.md"),
            "# garden\n\nFIX the fence",
        )
        .unwrap();
        app.reload(None).unwrap();

        // Title and body, ignoring case. Sections without a match are hidden.
        type_filter(&mut app, "fix");
        assert_eq!(
            listing(&app),
            ["[work]", "fix-login.md", "[home]", "garden.md"]
        );

        // A section name alone matches nothing.
        app.update(Action::FilterClear);
        type_filter(&mut app, "WORK");
        assert_eq!(listing(&app), Vec::<String>::new());

        app.update(Action::FilterClear);
        assert_eq!(listing(&app).len(), 7, "everything is back");
    }

    #[test]
    fn filter_keeps_cursor_on_a_matching_task() {
        let (_dir, mut app) = app_with_files(&["work/a-fix.md", "work/b.md", "work/c-fix.md"]);
        app.cursor = 3;
        type_filter(&mut app, "fix");
        assert_eq!(app.selected(), Some(Row::Task(0, 2)), "still on `c-fix`");

        // The task under the cursor no longer matches: first row.
        app.cursor = 1;
        app.update(Action::FilterBackspace);
        type_filter(&mut app, "xa");
        assert_eq!(app.cursor, 0);
        assert_eq!(listing(&app), Vec::<String>::new(), "no match, no rows");

        // Clearing keeps the cursor on its task, now at its full position.
        app.update(Action::FilterClear);
        type_filter(&mut app, "c-");
        app.cursor = 1;
        app.update(Action::FilterClear);
        assert_eq!(app.selected(), Some(Row::Task(0, 2)));
    }

    #[test]
    fn filter_delete_word() {
        let (_dir, mut app) = app_with_files(&["work/a.md"]);
        type_filter(&mut app, "a-fix bug ");
        app.update(Action::FilterDeleteWord);
        assert_eq!(app.filter, "a-fix ");
        app.update(Action::FilterDeleteWord);
        assert_eq!(app.filter, "");
        app.update(Action::FilterDeleteWord);
        assert_eq!(app.filter, "");
    }

    #[test]
    fn filter_input_takes_keys_until_accepted() {
        let (_dir, mut app) = app_with_files(&["work/a.md", "work/b.md"]);
        let key = |code| Event::Key(KeyEvent::from(code));
        app.handle_event(key(KeyCode::Char('/')));
        assert!(app.filter_input);

        // `j` is text here, not cursor down.
        app.handle_event(key(KeyCode::Char('j')));
        assert_eq!(app.filter, "j");
        app.handle_event(key(KeyCode::Backspace));
        app.handle_event(key(KeyCode::Char('b')));
        app.handle_event(key(KeyCode::Enter));
        assert!(!app.filter_input);
        assert_eq!(app.filter, "b", "accepting keeps the filter");

        // Back to normal keys, `Esc` clears.
        app.handle_event(key(KeyCode::Char('j')));
        assert_eq!(app.filter, "b");
        app.handle_event(key(KeyCode::Esc));
        assert_eq!(app.filter, "");
    }

    #[test]
    fn filter_blocks_move_and_new() {
        let (_dir, mut app) = app_with_files(&["work/a.md", "work/b.md"]);
        type_filter(&mut app, "a");
        app.update(Action::FilterAccept);
        app.cursor = 1;
        for action in [
            Action::MoveDown,
            Action::MoveUp,
            Action::NewBelow,
            Action::NewAbove,
            Action::NewSection,
        ] {
            app.status = None;
            app.update(action);
            assert_eq!(
                app.status,
                Some(Status::Info("clear the filter first (Esc)".to_string())),
                "{action:?}"
            );
        }
        assert_eq!(app.editor_request, None);
        app.update(Action::FilterClear);
        assert_eq!(listing(&app), ["[work]", "a.md", "b.md"]);
    }

    #[test]
    fn hint_goes_to_status() {
        let (_dir, mut app) = app_with_files(&[]);
        app.update(Action::Hint("use X"));
        assert_eq!(app.status, Some(Status::Info("use X".to_string())));
    }

    #[test]
    fn startup_normalizes_and_mentions_renames() {
        // Old numbered names, no order file yet.
        let dir = TempDir::new().unwrap();
        let work = dir.path().join("work");
        fs::create_dir_all(dir.path().join("01-home")).unwrap();
        fs::create_dir_all(&work).unwrap();
        fs::write(work.join("02-b.md"), "# b").unwrap();
        fs::write(work.join("01-a.md"), "# a").unwrap();
        fs::write(dir.path().join("01-home/old.md"), "# Old task").unwrap();
        let app = App::load(dir.path().to_path_buf(), None).unwrap();

        assert_eq!(
            listing(&app),
            ["[01-home]", "old-task.md", "[work]", "a.md", "b.md"]
        );
        assert_eq!(order_file(&dir), "# 01-home\nold-task\n\n# work\na\nb\n");
        assert_eq!(
            app.status,
            Some(Status::Info(
                "3 files renamed to match their titles".to_string()
            ))
        );
    }

    #[test]
    fn startup_cleans_the_order_file() {
        let (dir, _) = app_with_files(&["work/a.md", "work/b.md", "home/", "x/"]);
        fs::write(
            dir.path().join(order::FILE),
            "# x\n# work\ngone\nb\n# gone\n",
        )
        .unwrap();

        // Not listed: to the end, by name. Listed but gone: dropped. No rename, no message.
        let app = App::load(dir.path().to_path_buf(), None).unwrap();
        assert_eq!(listing(&app), ["[x]", "[work]", "b.md", "a.md", "[home]"]);
        assert_eq!(order_file(&dir), "# x\n\n# work\nb\na\n\n# home\n");
        assert_eq!(app.status, None);
    }

    #[test]
    fn reload_from_watcher_does_not_write() {
        let (dir, mut app) = app_with_files(&["work/a.md"]);
        fs::write(dir.path().join("work/draft.md"), "# New").unwrap();
        app.reload_keep_selection().unwrap();
        assert_eq!(listing(&app), ["[work]", "a.md", "draft.md"]);
        assert_eq!(order_file(&dir), "# work\na\n");
    }

    #[test]
    fn action_normalizes_first_and_says_so() {
        let (dir, mut app) = app_with_files(&["work/a.md", "work/b.md"]);
        fs::write(dir.path().join("work/draft.md"), "# New").unwrap();
        app.reload_keep_selection().unwrap();
        app.cursor = 1;

        app.update(Action::MoveDown);
        assert_eq!(listing(&app), ["[work]", "b.md", "a.md", "new.md"]);
        assert_eq!(
            app.status,
            Some(Status::Info(
                "1 file renamed to match its title".to_string()
            ))
        );

        // Undo only reverts the move, the rename stays.
        app.update(Action::Undo);
        assert_eq!(listing(&app), ["[work]", "a.md", "b.md", "new.md"]);
    }

    #[test]
    fn move_task_within_and_across_sections() {
        let (dir, mut app) = app_with_files(&["work/a.md", "work/b.md", "home/c.md"]);
        app.cursor = 1;

        app.update(Action::MoveDown);
        assert_eq!(listing(&app), ["[work]", "b.md", "a.md", "[home]", "c.md"]);
        assert_eq!(order_file(&dir), "# work\nb\na\n\n# home\nc\n");
        assert_eq!(app.cursor, 2, "cursor follows the task");

        app.update(Action::MoveDown);
        assert_eq!(listing(&app), ["[work]", "b.md", "[home]", "a.md", "c.md"]);
        assert!(dir.path().join("home/a.md").exists());
        assert_eq!(app.cursor, 3);

        // One undo per move, the cursor goes back with the task.
        app.update(Action::Undo);
        assert_eq!(listing(&app), ["[work]", "b.md", "a.md", "[home]", "c.md"]);
        assert_eq!(app.cursor, 2);
        app.update(Action::Undo);
        assert_eq!(app.cursor, 1);
        assert_eq!(listing(&app)[1], "a.md");
    }

    #[test]
    fn jumps() {
        let mut app = App::new(
            PathBuf::new(),
            sections(&[("work", &["a"]), ("home", &["b", "c"]), ("x", &["d"])]),
        );
        // Rows: work, a, home, b, c, x, d
        app.update(Action::Bottom);
        assert_eq!(app.cursor, 6);
        app.update(Action::PrevSection);
        assert_eq!(app.cursor, 5, "from a task to its own section");
        app.update(Action::PrevSection);
        assert_eq!(app.cursor, 2);
        app.update(Action::NextSection);
        assert_eq!(app.cursor, 5);
        app.update(Action::NextSection);
        assert_eq!(app.cursor, 5, "no next section: stays");
        app.update(Action::Top);
        assert_eq!(app.cursor, 0);
        app.update(Action::PrevSection);
        assert_eq!(app.cursor, 0, "nothing above the top");
    }

    #[test]
    fn jump_to_section() {
        let mut app = App::new(
            PathBuf::new(),
            sections(&[("work", &["a"]), ("home", &[]), ("x", &["b", "c"])]),
        );
        // Rows: work, a, home, x, b, c
        app.update(Action::JumpSection(2));
        assert_eq!(app.cursor, 4, "on the first task");
        app.update(Action::JumpSection(1));
        assert_eq!(app.cursor, 2, "empty section: on its header");
        app.update(Action::JumpSection(3));
        assert_eq!(app.cursor, 2, "no such section: stays");

        // The filter hides work and home, x keeps its number and only `c` shows.
        app.filter = "c".to_string();
        app.cursor = 0;
        app.update(Action::JumpSection(0));
        assert_eq!(
            app.selected(),
            Some(Row::Section(2)),
            "hidden section: stays"
        );
        app.update(Action::JumpSection(2));
        assert_eq!(app.selected(), Some(Row::Task(2, 1)));
    }

    #[test]
    fn move_task_stops_at_the_ends() {
        let (_dir, mut app) = app_with_files(&["work/a.md", "home/b.md"]);
        app.cursor = 1;
        app.update(Action::MoveUp);
        app.cursor = 3;
        app.update(Action::MoveDown);
        assert_eq!(listing(&app), ["[work]", "a.md", "[home]", "b.md"]);
        assert_eq!(app.status, None);
    }

    #[test]
    fn move_section_with_its_tasks() {
        let (dir, mut app) = app_with_files(&["work/a.md", "home/b.md", "x/"]);

        app.update(Action::MoveDown);
        assert_eq!(listing(&app), ["[home]", "b.md", "[work]", "a.md", "[x]"]);
        assert_eq!(app.cursor, 2, "cursor follows the section");
        assert_eq!(order_file(&dir), "# home\nb\n\n# work\na\n\n# x\n");

        app.update(Action::Undo);
        assert_eq!(app.sections[0].name, "work");
        assert_eq!(app.cursor, 0, "cursor goes back with the section");

        // Nothing above the first one.
        app.update(Action::MoveUp);
        assert_eq!(app.sections[0].name, "work");
        assert_eq!(app.status, None);
    }

    #[test]
    fn done_archives_and_undo_restores() {
        let (dir, mut app) = app_with_files(&["work/a.md", "home/b.md"]);
        let today = jiff::Zoned::now().date().to_string();
        let archive_path = dir.path().join(".archive").join(format!("{today}.md"));

        app.cursor = 1;
        app.update(Action::Done);
        app.cursor = 2;
        app.update(Action::Done);
        assert_eq!(
            app.status,
            Some(Status::Info("task archived (u to undo)".to_string()))
        );
        assert_eq!(app.done_today(), 2);
        assert_eq!(listing(&app), ["[work]", "[home]"]);
        assert_eq!(order_file(&dir), "# work\n\n# home\n");
        assert_eq!(
            fs::read_to_string(&archive_path).unwrap(),
            format!("# {today}\n\n## work\n\n### a\n\n## home\n\n### b\n")
        );

        app.update(Action::Undo);
        assert_eq!(listing(&app), ["[work]", "[home]", "b.md"]);
        assert_eq!(app.done_today(), 1);
        assert_eq!(
            fs::read_to_string(&archive_path).unwrap(),
            format!("# {today}\n\n## work\n\n### a\n")
        );

        app.update(Action::Undo);
        assert_eq!(listing(&app), ["[work]", "a.md", "[home]", "b.md"]);
        assert!(!archive_path.exists());
    }
}
