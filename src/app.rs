//! App state, the run loop, and what each [`Action`] does to it and to the disk.

use std::{
    fs, io,
    path::{Path, PathBuf},
    time::Duration,
};

use color_eyre::{
    Report, Result,
    eyre::{WrapErr, eyre},
};
use ratatui::{
    DefaultTerminal,
    crossterm::event::{self, Event, KeyEventKind},
};

use crate::{
    edit, editor, filter,
    keymap::{self, Action, Keymap},
    load,
    name::MAX_ORDER,
    op::{self, Op},
    task::{Section, Task},
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
}

/// Work that needs the editor. Set by `update`, handled by the run loop, which owns
/// the terminal. Keeps `update` testable without a terminal.
#[derive(Debug, Clone, PartialEq, Eq)]
enum EditorRequest {
    /// `<CR>`: edit an existing task.
    Open(PathBuf),
    /// `n`: write a new task, inserted at `index` in section `section`.
    Create { section: usize, index: usize },
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
    /// Directory with `todo/` and `archive/`.
    root: PathBuf,
    pub sections: Vec<Section>,
    /// Index into [`App::rows`].
    pub cursor: usize,
    pub keymap: Keymap,
    /// Applied actions, newest last. Undo applies the inverse of the last one.
    undo: Vec<UndoEntry>,
    pub status: Option<Status>,
    /// Help overlay is open.
    pub show_help: bool,
    /// Renames done at startup, relative to `todo/`. Shown as a popup until the next key.
    pub renamed: Vec<(PathBuf, PathBuf)>,
    editor_request: Option<EditorRequest>,
    /// `/` filter, empty for none. Applied in [`App::rows`], so everything that works on rows
    /// sees only what matches.
    pub filter: String,
    /// The filter input at the bottom is open and takes the keys.
    pub filter_input: bool,
    /// First list line on screen. Kept between frames so the view only scrolls when the
    /// cursor would leave it, see [`ui::render`].
    pub list_offset: usize,
}

impl App {
    pub fn new(root: PathBuf, sections: Vec<Section>) -> Self {
        Self {
            running: true,
            root,
            sections,
            cursor: 0,
            keymap: Keymap::default(),
            undo: Vec::new(),
            status: None,
            show_help: false,
            renamed: Vec::new(),
            editor_request: None,
            filter: String::new(),
            filter_input: false,
            list_offset: 0,
        }
    }

    /// Loads `todo/` and normalizes it. Renames are listed in a popup, since files changing
    /// their names on start can be a surprise.
    pub fn load(root: PathBuf) -> Result<Self> {
        let todo_dir = root.join("todo");
        let sections = load::load(&todo_dir)?;
        let mut app = Self::new(root, sections);
        match app.normalize() {
            Ok(renames) => {
                let relative = |path: PathBuf| {
                    path.strip_prefix(&todo_dir)
                        .map(Path::to_path_buf)
                        .unwrap_or(path)
                };
                app.renamed = renames
                    .into_iter()
                    .map(|(from, to)| (relative(from), relative(to)))
                    .collect();
            }
            Err(err) => app.report(Err(err)),
        }
        Ok(app)
    }

    /// Main loop: draw if needed, wait briefly for a key, check for outside changes.
    ///
    /// Keys are read on this thread (no background reader), so the TUI can hand the
    /// terminal to `$EDITOR` without the two fighting over stdin.
    pub fn run(mut self, terminal: &mut DefaultTerminal) -> Result<()> {
        let mut watcher = TodoWatcher::new(&self.root.join("todo"))?;
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
                let result = match request {
                    EditorRequest::Open(path) => {
                        editor::open(terminal, &path, &[]).and_then(|()| self.after_edit())
                    }
                    EditorRequest::Create { section, index } => {
                        editor::create(terminal, &self.sections[section].dir)
                            .and_then(|content| self.after_create(section, index, &content))
                    }
                };
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

    /// Path behind a row: the task file or the section directory.
    fn row_path(&self, row: Row) -> PathBuf {
        match row {
            Row::Section(s) => self.sections[s].dir.clone(),
            Row::Task(s, t) => self.sections[s].tasks[t].path.clone(),
        }
    }

    fn handle_event(&mut self, event: Event) {
        // Only presses: some terminals also report key releases.
        if let Event::Key(key) = event
            && key.kind == KeyEventKind::Press
        {
            self.status = None;
            // Any key closes the help and does nothing else, so a stray key cannot change
            // tasks that are hidden behind the overlay.
            if self.show_help || !self.renamed.is_empty() {
                self.show_help = false;
                self.renamed.clear();
                return;
            }
            let action = if self.filter_input {
                keymap::filter_input(key)
            } else {
                self.keymap.handle(key)
            };
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
        // Where a task goes is unclear while tasks in between are hidden.
        if !self.filter.is_empty()
            && matches!(action, Action::MoveDown | Action::MoveUp | Action::New)
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
            Action::MoveDown | Action::MoveUp => self.move_selected(action == Action::MoveDown)?,
            Action::Open => {
                if let Some(Row::Task(s, t)) = self.selected() {
                    let path = self.sections[s].tasks[t].path.clone();
                    self.editor_request = Some(EditorRequest::Open(path));
                }
            }
            Action::New => {
                // At the end of the cursor's section, from its header or any of its tasks.
                if let Some(Row::Section(section) | Row::Task(section, _)) = self.selected() {
                    let index = self.sections[section].tasks.len();
                    self.editor_request = Some(EditorRequest::Create { section, index });
                }
            }
            Action::Done => self.done()?,
            Action::Delete => self.delete()?,
            Action::Undo => self.undo()?,
            Action::Help => self.show_help = !self.show_help,
            Action::Quit => self.running = false,
            Action::Hint(hint) => self.status = Some(Status::Info(hint.to_string())),
            Action::Filter => self.filter_input = true,
            Action::FilterType(c) => self.set_filter(|filter| filter.push(c)),
            Action::FilterBackspace => self.set_filter(|filter| {
                filter.pop();
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

    /// Reloads after the editor closed. Normalizing renames the file if its heading changed.
    fn after_edit(&mut self) -> Result<()> {
        self.reload(None)?;
        self.normalize_before_action();
        Ok(())
    }

    /// Renames all tasks into shape with [`edit::normalize`] and returns the renames.
    ///
    /// Not added to undo: undoing it would only be redone by the next action. The cursor
    /// follows a renamed task.
    fn normalize(&mut self) -> Result<Vec<(PathBuf, PathBuf)>> {
        // Tasks first: their renames use the current directory paths.
        let mut ops: Vec<Op> = self.sections.iter().flat_map(edit::normalize).collect();
        ops.extend(edit::normalize_sections(&self.sections));
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
            let mut path = self.row_path(row);
            // Follow each rename, also of the directory the selected task is in.
            for (from, to) in &renames {
                if let Ok(rest) = path.strip_prefix(from) {
                    path = to.join(rest);
                }
            }
            path
        });
        op::apply_all(&ops)?;
        self.reload(selected.as_deref())?;
        Ok(renames)
    }

    /// [`Self::normalize`] before an action that writes, so the action sees clean names.
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

    /// Inserts what was written in the editor for `n`.
    fn after_create(&mut self, section: usize, index: usize, content: &str) -> Result<()> {
        let task = Task::parse(PathBuf::from("new.md"), content);
        if task.heading.is_none() {
            return Err(eyre!("aborted, empty title"));
        }
        // Renames keep the order of tasks and sections, so `section` and `index` stay valid.
        self.normalize_before_action();
        // `sections` is from before the editor. If the files changed meanwhile, the ops refuse
        // and the text is lost, so say that clearly.
        self.insert(section, index, &task)
            .map_err(|err| eyre!("task not added: {err}"))
    }

    fn done(&mut self) -> Result<()> {
        let Some(Row::Task(s, t)) = self.selected() else {
            return Ok(());
        };
        let date = jiff::Zoned::now().date().to_string();
        let archive_dir = self.root.join("archive");
        let archive_path = archive_dir.join(format!("{date}.md"));
        // Not an op: an empty `archive/` left behind by undo does no harm.
        fs::create_dir_all(&archive_dir)
            .wrap_err_with(|| format!("failed to create {}", archive_dir.display()))?;
        let existing = read_if_exists(&archive_path)?;

        let ops = edit::archive(
            &self.sections[s],
            t,
            &archive_path,
            existing.as_deref(),
            &date,
        );
        self.apply(ops)
    }

    /// `J` / `K`: moves the task or section under the cursor one place, the cursor follows.
    /// Nothing happens at the edges.
    fn move_selected(&mut self, down: bool) -> Result<()> {
        let (ops, moved_to) = match self.selected() {
            Some(Row::Task(s, t)) => {
                let Some((to, index)) = self.task_target(s, t, down) else {
                    return Ok(());
                };
                edit::move_task(&self.sections, s, t, to, index).ok_or_else(section_full)?
            }
            Some(Row::Section(s)) => match edit::move_section(&self.sections, s, down) {
                Some(result) => result,
                None => return Ok(()),
            },
            None => return Ok(()),
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
            .find(|&i| matches!(rows[i], Row::Section(_)))
            .unwrap_or(0)
    }

    /// `}`: the next section header below the cursor, the last row if there is none (like vim).
    fn next_section_row(&self) -> usize {
        let rows = self.rows();
        (self.cursor + 1..rows.len())
            .find(|&i| matches!(rows[i], Row::Section(_)))
            .unwrap_or(rows.len().saturating_sub(1))
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
            None => return Ok(()),
        };
        let title = self.sections[s].tasks[t].title();
        // Cursor stays on the same row, which is now the next task (like vim).
        self.apply(edit::delete(&self.sections[s], t))?;
        self.status = Some(Status::Info(format!("deleted {title} (u to undo)")));
        Ok(())
    }

    /// Only empty sections, so `D` never takes tasks with it. The inbox is `todo/` itself.
    fn delete_section(&mut self, s: usize) -> Result<()> {
        let section = &self.sections[s];
        let title = section.title();
        if s == 0 {
            return Err(eyre!("the inbox can't be deleted"));
        }
        if !section.tasks.is_empty() {
            return Err(eyre!("{title} is not empty"));
        }
        // Cursor stays on the same row, which is now the next section.
        self.apply(edit::delete_section(&self.sections, s))?;
        self.status = Some(Status::Info(format!("deleted {title} (u to undo)")));
        Ok(())
    }

    /// Writes `task` into section `s` at `index` and puts the cursor on it.
    fn insert(&mut self, s: usize, index: usize, task: &Task) -> Result<()> {
        let (ops, created) =
            edit::insert(&self.sections[s], index, task).ok_or_else(section_full)?;
        self.apply(ops)?;
        self.select_path(&created);
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

    /// Reads `todo/` again and puts the cursor on `select` if it still exists, else keeps the
    /// row number, clamped. The disk is the source of truth, `sections` is only a view of it.
    fn reload(&mut self, select: Option<&Path>) -> Result<()> {
        self.sections = load::load(&self.root.join("todo"))?;
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

    /// All rows in display order: each section header followed by its tasks.
    ///
    /// With a filter only the tasks whose title or body matches, under their section header.
    /// Section names don't match: a section is shown for its tasks only, empty ones without a
    /// filter.
    ///
    /// Computed on demand instead of stored, so it can never be out of sync with `sections`.
    pub fn rows(&self) -> Vec<Row> {
        let mut rows = Vec::new();
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
}

/// File content, `None` if the file does not exist.
fn read_if_exists(path: &Path) -> Result<Option<String>> {
    match fs::read_to_string(path) {
        Ok(content) => Ok(Some(content)),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(err).wrap_err_with(|| format!("failed to read {}", path.display())),
    }
}

fn section_full() -> Report {
    eyre!("section is full ({MAX_ORDER} tasks)")
}

fn renamed_message(count: usize) -> String {
    if count == 1 {
        "renamed 1 file to match title and order".to_string()
    } else {
        format!("renamed {count} files to match titles and order")
    }
}

#[cfg(test)]
pub mod tests {
    use std::fs;

    use ratatui::crossterm::event::{KeyCode, KeyEvent};
    use tempfile::TempDir;

    use super::*;

    /// Sections with the given names (empty = inbox) and task titles. Nothing on disk.
    pub fn sections(spec: &[(&[&str], &[&str])]) -> Vec<Section> {
        spec.iter()
            .map(|(path, tasks)| Section {
                path: path.iter().map(|s| s.to_string()).collect(),
                dir: PathBuf::new(),
                tasks: tasks
                    .iter()
                    .map(|title| Task::parse(PathBuf::new(), &format!("# {title}")))
                    .collect(),
            })
            .collect()
    }

    /// An app on a temp root with the given files in `todo/`. Paths ending in `/` become directories.
    /// Each file gets its slug as heading: `01-a.md` -> `# a`.
    fn app_with_files(files: &[&str]) -> (TempDir, App) {
        let dir = TempDir::new().unwrap();
        fs::create_dir(dir.path().join("todo")).unwrap();
        for file in files {
            let path = dir.path().join("todo").join(file);
            if file.ends_with('/') {
                fs::create_dir_all(path).unwrap();
            } else {
                fs::create_dir_all(path.parent().unwrap()).unwrap();
                let stem = path.file_stem().unwrap().to_str().unwrap();
                let title = stem.split_once('-').map_or(stem, |(_, slug)| slug);
                fs::write(&path, format!("# {title}")).unwrap();
            }
        }
        let app = App::load(dir.path().to_path_buf()).unwrap();
        (dir, app)
    }

    /// File names on disk in row order, sections as `[name]`.
    fn listing(app: &App) -> Vec<String> {
        app.rows()
            .into_iter()
            .map(|row| match row {
                Row::Section(s) => format!("[{}]", app.sections[s].title()),
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
            sections(&[(&[], &["a"]), (&["work"], &[]), (&["x"], &["b", "c"])]),
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
        let mut app = App::new(PathBuf::new(), sections(&[(&[], &["a"])]));
        app.update(Action::Up);
        assert_eq!(app.cursor, 0);
        app.update(Action::Down);
        app.update(Action::Down);
        assert_eq!(app.cursor, 1);
    }

    #[test]
    fn any_key_closes_help_without_acting() {
        let (_dir, mut app) = app_with_files(&["01-a.md"]);
        app.cursor = 1;
        app.update(Action::Help);
        assert!(app.show_help);

        app.handle_event(Event::Key(KeyCode::Char('x').into()));
        assert!(!app.show_help);
        assert_eq!(listing(&app), ["[inbox]", "01-a.md"], "x did not archive");
    }

    #[test]
    fn reload_keeps_cursor_on_the_same_task() {
        let (dir, mut app) = app_with_files(&["01-a.md", "02-b.md"]);
        app.cursor = 2;

        // Changes from outside: a task appears above, one disappears.
        let todo = dir.path().join("todo");
        fs::write(todo.join("01-z.md"), "# z").unwrap();
        fs::write(todo.join("00-y.md"), "# y").unwrap();
        fs::remove_file(todo.join("01-a.md")).unwrap();
        app.reload_keep_selection().unwrap();

        assert_eq!(listing(&app), ["[inbox]", "00-y.md", "01-z.md", "02-b.md"]);
        assert_eq!(app.cursor, 3, "still on 02-b.md");
    }

    #[test]
    fn reload_clamps_cursor_if_task_is_gone() {
        let (dir, mut app) = app_with_files(&["01-a.md"]);
        app.cursor = 1;
        fs::remove_file(dir.path().join("todo/01-a.md")).unwrap();
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
        let (_dir, mut app) = app_with_files(&["01-a.md", "02-b.md"]);
        app.cursor = 1;

        app.update(Action::Delete);
        assert_eq!(listing(&app), ["[inbox]", "01-b.md"], "the gap is closed");
        assert_eq!(app.cursor, 1, "cursor moves onto the next task");
        assert_eq!(
            app.status,
            Some(Status::Info("deleted a (u to undo)".to_string()))
        );

        app.update(Action::Undo);
        assert_eq!(listing(&app), ["[inbox]", "01-a.md", "02-b.md"]);
        assert_eq!(app.cursor, 1, "cursor is on the restored task");

        app.update(Action::Undo);
        assert_eq!(
            app.status,
            Some(Status::Error("nothing to undo".to_string()))
        );
    }

    #[test]
    fn delete_section_only_if_empty_and_not_inbox() {
        let (dir, mut app) = app_with_files(&["01-a.md", "01-work/", "02-home/01-b.md"]);
        let before = listing(&app);

        app.update(Action::Delete);
        assert_eq!(
            app.status,
            Some(Status::Error("the inbox can't be deleted".to_string()))
        );
        app.cursor = 3;
        app.update(Action::Delete);
        assert_eq!(
            app.status,
            Some(Status::Error("home is not empty".to_string()))
        );
        assert_eq!(listing(&app), before);

        // The empty `work` goes, `home` moves up into its place, under the cursor.
        app.cursor = 2;
        app.update(Action::Delete);
        assert_eq!(listing(&app), ["[inbox]", "01-a.md", "[home]", "01-b.md"]);
        assert!(dir.path().join("todo/01-home").is_dir());
        assert_eq!(app.selected(), Some(Row::Section(1)));
        assert_eq!(
            app.status,
            Some(Status::Info("deleted work (u to undo)".to_string()))
        );

        app.update(Action::Undo);
        assert_eq!(listing(&app), before);
        assert_eq!(app.cursor, 2, "back on `work`");
    }

    /// Types `text` into the filter input, like `/` followed by the keys.
    fn type_filter(app: &mut App, text: &str) {
        app.update(Action::Filter);
        for c in text.chars() {
            app.update(Action::FilterType(c));
        }
    }

    #[test]
    fn filter_shows_matching_tasks_under_their_section() {
        let (dir, mut app) = app_with_files(&[
            "01-fix-login.md",
            "02-buy-milk.md",
            "01-work/01-review.md",
            "02-home/01-garden.md",
        ]);
        fs::write(
            dir.path().join("todo/02-home/01-garden.md"),
            "# garden\n\nFIX the fence",
        )
        .unwrap();
        app.reload(None).unwrap();

        // Title and body, ignoring case. Sections without a match are hidden.
        type_filter(&mut app, "fix");
        assert_eq!(
            listing(&app),
            ["[inbox]", "01-fix-login.md", "[home]", "01-garden.md"]
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
        let (_dir, mut app) = app_with_files(&["01-a-fix.md", "02-b.md", "03-c-fix.md"]);
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
    fn filter_input_takes_keys_until_accepted() {
        let (_dir, mut app) = app_with_files(&["01-a.md", "02-b.md"]);
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
        let (_dir, mut app) = app_with_files(&["01-a.md", "02-b.md"]);
        type_filter(&mut app, "a");
        app.update(Action::FilterAccept);
        app.cursor = 1;
        for action in [Action::MoveDown, Action::MoveUp, Action::New] {
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
        assert_eq!(listing(&app), ["[inbox]", "01-a.md", "02-b.md"]);
    }

    #[test]
    fn hint_goes_to_status() {
        let (_dir, mut app) = app_with_files(&[]);
        app.update(Action::Hint("use X"));
        assert_eq!(app.status, Some(Status::Info("use X".to_string())));
    }

    #[test]
    fn open_requests_editor_for_task_only() {
        let (_dir, mut app) = app_with_files(&["01-a.md"]);
        app.update(Action::Open);
        assert_eq!(app.editor_request, None, "section header opens nothing");

        app.cursor = 1;
        app.update(Action::Open);
        let Some(EditorRequest::Open(path)) = app.editor_request.clone() else {
            panic!("expected an open request");
        };
        assert!(path.ends_with("todo/01-a.md"));
    }

    #[test]
    fn new_requests_create_at_section_end() {
        let (_dir, mut app) = app_with_files(&["01-a.md", "02-b.md", "01-work/01-c.md"]);
        let at_end = |section| Some(EditorRequest::Create { section, index: 2 });

        // On a task and on the header of the same section.
        for cursor in [1, 0] {
            app.cursor = cursor;
            app.update(Action::New);
            assert_eq!(app.editor_request, at_end(0), "cursor {cursor}");
        }

        app.cursor = 3;
        app.update(Action::New);
        assert_eq!(
            app.editor_request,
            Some(EditorRequest::Create {
                section: 1,
                index: 1
            })
        );
    }

    #[test]
    fn after_create_inserts_and_undo_removes() {
        let (_dir, mut app) = app_with_files(&["01-a.md", "02-b.md"]);
        // `n` was pressed on `a`.
        app.cursor = 1;
        app.after_create(0, 2, "# New task\n\nnotes\n").unwrap();
        assert_eq!(
            listing(&app),
            ["[inbox]", "01-a.md", "02-b.md", "03-new-task.md"]
        );
        assert_eq!(app.cursor, 3, "cursor is on the new task");

        app.update(Action::Undo);
        assert_eq!(listing(&app), ["[inbox]", "01-a.md", "02-b.md"]);
        assert_eq!(
            app.cursor, 1,
            "cursor is back on `a`, where `n` was pressed"
        );
    }

    #[test]
    fn after_create_aborts_without_title() {
        let (_dir, mut app) = app_with_files(&["01-a.md"]);
        for content in ["# \n", "", "text but no heading\n"] {
            let err = app.after_create(0, 1, content).unwrap_err();
            assert_eq!(err.to_string(), "aborted, empty title");
        }
        assert_eq!(listing(&app), ["[inbox]", "01-a.md"]);
    }

    #[test]
    fn after_create_fails_on_changes_made_while_editing() {
        let (dir, mut app) = app_with_files(&["01-a.md", "02-b.md"]);
        app.cursor = 1;
        // A sync renames `b` while the editor is open.
        let todo = dir.path().join("todo");
        fs::rename(todo.join("02-b.md"), todo.join("02-c.md")).unwrap();

        let err = app.after_create(0, 1, "# New").unwrap_err();
        assert!(err.to_string().starts_with("task not added: "), "{err}");
        assert_eq!(
            fs::read_dir(&todo).unwrap().count(),
            2,
            "nothing written, `01-a.md` and `02-c.md` only"
        );
    }

    #[test]
    fn after_edit_renames_to_new_heading() {
        let (_dir, mut app) = app_with_files(&["01-a.md", "02-b.md"]);
        app.cursor = 1;
        let path = app.sections[0].tasks[0].path.clone();

        // What the user does in the editor.
        fs::write(&path, "# Renamed task\n\nnotes").unwrap();
        app.after_edit().unwrap();

        assert_eq!(listing(&app), ["[inbox]", "01-renamed-task.md", "02-b.md"]);
        assert_eq!(app.cursor, 1, "cursor follows the renamed file");
        assert_eq!(app.sections[0].tasks[0].body, "notes");
    }

    #[test]
    fn startup_normalizes_and_lists_renames() {
        let (_dir, mut app) = app_with_files(&["01-a.md", "b.md", "01-work/old.md"]);
        assert_eq!(
            listing(&app),
            ["[inbox]", "01-a.md", "02-b.md", "[work]", "01-old.md"]
        );
        assert_eq!(
            app.renamed,
            [
                ("b.md".into(), "02-b.md".into()),
                ("01-work/old.md".into(), "01-work/01-old.md".into()),
            ]
        );

        // Any key closes the popup and does nothing else.
        app.handle_event(Event::Key(KeyCode::Char('j').into()));
        assert!(app.renamed.is_empty());
        assert_eq!(app.cursor, 0);
    }

    #[test]
    fn startup_cleans_section_dirs_and_cursor_follows() {
        let (_dir, mut app) = app_with_files(&["005-work/01-a.md", "017-home/"]);
        assert_eq!(
            app.renamed,
            [
                ("005-work".into(), "01-work".into()),
                ("017-home".into(), "02-home".into()),
            ]
        );
        assert!(app.sections[1].dir.ends_with("01-work"));

        // An action normalizes too, the cursor stays on the task inside the renamed dir.
        let work = app.sections[1].dir.clone();
        fs::rename(&work, work.with_file_name("001-work")).unwrap();
        app.reload_keep_selection().unwrap();
        app.cursor = 2;
        app.update(Action::MoveDown);
        assert_eq!(listing(&app), ["[inbox]", "[work]", "[home]", "01-a.md"]);
        assert_eq!(app.cursor, 3, "cursor follows the task");
    }

    #[test]
    fn reload_from_watcher_does_not_rename() {
        let (dir, mut app) = app_with_files(&["01-a.md"]);
        fs::write(dir.path().join("todo/new.md"), "# New").unwrap();
        app.reload_keep_selection().unwrap();
        assert_eq!(listing(&app), ["[inbox]", "01-a.md", "new.md"]);
    }

    #[test]
    fn action_normalizes_first_and_says_so() {
        let (dir, mut app) = app_with_files(&["01-a.md", "02-b.md"]);
        fs::write(dir.path().join("todo/new.md"), "# New").unwrap();
        app.reload_keep_selection().unwrap();
        app.cursor = 1;

        app.update(Action::MoveDown);
        assert_eq!(
            listing(&app),
            ["[inbox]", "01-b.md", "02-a.md", "03-new.md"]
        );
        assert_eq!(
            app.status,
            Some(Status::Info(
                "renamed 1 file to match title and order".to_string()
            ))
        );

        // Undo only reverts the move, the rename stays.
        app.update(Action::Undo);
        assert_eq!(
            listing(&app),
            ["[inbox]", "01-a.md", "02-b.md", "03-new.md"]
        );
    }

    #[test]
    fn move_task_within_and_across_sections() {
        let (_dir, mut app) = app_with_files(&["01-a.md", "02-b.md", "01-work/01-c.md"]);
        app.cursor = 1;

        app.update(Action::MoveDown);
        assert_eq!(
            listing(&app),
            ["[inbox]", "01-b.md", "02-a.md", "[work]", "01-c.md"]
        );
        assert_eq!(app.cursor, 2, "cursor follows the task");

        app.update(Action::MoveDown);
        assert_eq!(
            listing(&app),
            ["[inbox]", "01-b.md", "[work]", "01-a.md", "02-c.md"]
        );
        assert_eq!(app.cursor, 3);

        // One undo per move, the cursor goes back with the task.
        app.update(Action::Undo);
        assert_eq!(
            listing(&app),
            ["[inbox]", "01-b.md", "02-a.md", "[work]", "01-c.md"]
        );
        assert_eq!(app.cursor, 2);
        app.update(Action::Undo);
        assert_eq!(app.cursor, 1);
        assert_eq!(listing(&app)[1], "01-a.md");
    }

    #[test]
    fn jumps() {
        let mut app = App::new(
            PathBuf::new(),
            sections(&[(&[], &["a"]), (&["work"], &["b", "c"]), (&["x"], &["d"])]),
        );
        // Rows: inbox, a, work, b, c, x, d
        app.update(Action::Bottom);
        assert_eq!(app.cursor, 6);
        app.update(Action::PrevSection);
        assert_eq!(app.cursor, 5, "from a task to its own section");
        app.update(Action::PrevSection);
        assert_eq!(app.cursor, 2);
        app.update(Action::NextSection);
        assert_eq!(app.cursor, 5);
        app.update(Action::NextSection);
        assert_eq!(app.cursor, 6, "no next section: last row");
        app.update(Action::Top);
        assert_eq!(app.cursor, 0);
        app.update(Action::PrevSection);
        assert_eq!(app.cursor, 0, "nothing above the top");
    }

    #[test]
    fn move_task_stops_at_the_ends() {
        let (_dir, mut app) = app_with_files(&["01-a.md", "01-work/01-b.md"]);
        app.cursor = 1;
        app.update(Action::MoveUp);
        app.cursor = 3;
        app.update(Action::MoveDown);
        assert_eq!(listing(&app), ["[inbox]", "01-a.md", "[work]", "01-b.md"]);
        assert_eq!(app.status, None);
    }

    #[test]
    fn move_section_with_its_tasks() {
        let (_dir, mut app) = app_with_files(&["01-work/01-a.md", "02-home/01-b.md", "03-x/"]);
        app.cursor = 1;

        app.update(Action::MoveDown);
        assert_eq!(
            listing(&app),
            ["[inbox]", "[home]", "01-b.md", "[work]", "01-a.md", "[x]"]
        );
        assert_eq!(app.cursor, 3, "cursor follows the section");
        assert!(app.sections[1].dir.ends_with("01-home"));
        assert!(app.sections[2].dir.ends_with("02-work"));

        app.update(Action::Undo);
        assert!(app.sections[1].dir.ends_with("01-work"));
        assert_eq!(app.cursor, 1, "cursor goes back with the section");

        // The inbox never moves.
        app.cursor = 0;
        app.update(Action::MoveDown);
        assert_eq!(app.sections[1].title(), "work");
    }

    #[test]
    fn done_archives_and_undo_restores() {
        let (dir, mut app) = app_with_files(&["01-a.md", "01-work/01-b.md"]);
        let today = jiff::Zoned::now().date().to_string();
        let archive_path = dir.path().join("archive").join(format!("{today}.md"));

        app.cursor = 1;
        app.update(Action::Done);
        app.cursor = 2;
        app.update(Action::Done);
        assert_eq!(app.status, None);
        assert_eq!(listing(&app), ["[inbox]", "[work]"]);
        assert_eq!(
            fs::read_to_string(&archive_path).unwrap(),
            format!("# {today}\n\n## inbox\n\n### a\n\n## work\n\n### b\n")
        );

        app.update(Action::Undo);
        assert_eq!(listing(&app), ["[inbox]", "[work]", "01-b.md"]);
        assert_eq!(
            fs::read_to_string(&archive_path).unwrap(),
            format!("# {today}\n\n## inbox\n\n### a\n")
        );

        app.update(Action::Undo);
        assert_eq!(listing(&app), ["[inbox]", "01-a.md", "[work]", "01-b.md"]);
        assert!(!archive_path.exists());
    }

    #[test]
    fn insert_renumbers_and_undo_reverts_it() {
        let (_dir, mut app) = app_with_files(&["01-a.md", "011-b.md", "012-c.md"]);
        let before = listing(&app);

        // Prefixes with gaps are made dense along with the insert.
        app.cursor = 1;
        app.after_create(0, 3, "# New").unwrap();
        assert_eq!(
            listing(&app),
            ["[inbox]", "01-a.md", "02-b.md", "03-c.md", "04-new.md"]
        );
        assert_eq!(app.cursor, 4, "cursor is on the new task");

        // One undo reverts the insert including the renumbering.
        app.update(Action::Undo);
        assert_eq!(listing(&app), before);
        assert_eq!(
            app.cursor, 1,
            "cursor is back on `a`, where `n` was pressed"
        );
    }
}
