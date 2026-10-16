use std::{
    fs, io,
    path::{Path, PathBuf},
    time::Duration,
};

use color_eyre::{
    Result,
    eyre::{WrapErr, eyre},
};
use ratatui::{
    DefaultTerminal,
    crossterm::event::{self, Event, KeyEventKind},
};

use crate::{
    edit, editor,
    keymap::{Action, Keymap},
    load,
    op::{self, Op},
    task::{Section, Task},
    ui,
    watch::TodoWatcher,
};

/// How long to wait for a key before checking the file watcher again.
const POLL_INTERVAL: Duration = Duration::from_millis(50);

/// One line in the list. The cursor moves over these.
///
/// Section headers are rows too, so the cursor can land on an empty section (to paste into it).
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
    /// `o` / `O`: write a new task, inserted at `index` in section `section`.
    Create { section: usize, index: usize },
}

/// Message for the status line, cleared on the next key press.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    Error(String),
    Info(String),
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
    /// Last cut task, in memory only. Pasting does not clear it (like vim).
    register: Option<Task>,
    /// Applied op lists, newest last. Undo applies the inverse of the last one.
    undo: Vec<Vec<Op>>,
    pub status: Option<Status>,
    /// Help overlay is open.
    pub show_help: bool,
    /// Renames done at startup, relative to `todo/`. Shown as a popup until the next key.
    pub renamed: Vec<(PathBuf, PathBuf)>,
    editor_request: Option<EditorRequest>,
}

impl App {
    pub fn new(root: PathBuf, sections: Vec<Section>) -> Self {
        Self {
            running: true,
            root,
            sections,
            cursor: 0,
            keymap: Keymap::default(),
            register: None,
            undo: Vec::new(),
            status: None,
            show_help: false,
            renamed: Vec::new(),
            editor_request: None,
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
                terminal.draw(|frame| ui::render(&self, frame))?;
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
        self.reload()?;
        self.clamp_cursor();
        if let Some(path) = selected {
            self.select_path(&path);
        }
        Ok(())
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
            if let Some(action) = self.keymap.handle(key) {
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
        match action {
            Action::Up => self.cursor = self.cursor.saturating_sub(1),
            Action::Down => {
                let last = self.rows().len().saturating_sub(1);
                self.cursor = (self.cursor + 1).min(last);
            }
            Action::MoveDown | Action::MoveUp => {
                self.normalize_before_action();
                self.move_selected(action == Action::MoveDown)?;
            }
            Action::Open => {
                if let Some(Row::Task(s, t)) = self.selected() {
                    let path = self.sections[s].tasks[t].path.clone();
                    self.editor_request = Some(EditorRequest::Open(path));
                }
            }
            Action::NewBelow | Action::NewAbove => {
                if let Some((section, index)) = self.insert_position(action == Action::NewAbove) {
                    self.editor_request = Some(EditorRequest::Create { section, index });
                }
            }
            Action::Done => {
                self.normalize_before_action();
                self.done()?;
            }
            Action::Cut => {
                self.normalize_before_action();
                self.cut()?;
            }
            Action::PasteBelow | Action::PasteAbove => {
                self.normalize_before_action();
                self.paste(action == Action::PasteAbove)?;
            }
            Action::Undo => self.undo()?,
            Action::Help => self.show_help = !self.show_help,
            Action::Quit => self.running = false,
        }
        Ok(())
    }

    /// Reloads after the editor closed. Normalizing renames the file if its heading changed.
    fn after_edit(&mut self) -> Result<()> {
        self.reload()?;
        self.clamp_cursor();
        self.normalize_before_action();
        Ok(())
    }

    /// Renames all tasks into shape with [`edit::normalize`] and returns the renames.
    ///
    /// Not added to undo: undoing it would only be redone by the next action. The cursor
    /// follows a renamed task.
    fn normalize(&mut self) -> Result<Vec<(PathBuf, PathBuf)>> {
        let ops: Vec<Op> = self.sections.iter().flat_map(edit::normalize).collect();
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

        let selected = self.selected().map(|row| self.row_path(row));
        op::apply_all(&ops)?;
        self.reload()?;
        self.clamp_cursor();
        if let Some(path) = selected {
            let path = renames
                .iter()
                .find(|(from, _)| *from == path)
                .map_or(path, |(_, to)| to.clone());
            self.select_path(&path);
        }
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

    /// Inserts what was written in the editor for `o` / `O`, like a paste.
    fn after_create(&mut self, section: usize, index: usize, content: &str) -> Result<()> {
        let task = Task::parse(PathBuf::from("new.md"), content);
        if task.heading.is_none() {
            return Err(eyre!("aborted, empty title"));
        }
        // Renames keep the order of tasks and sections, so `section` and `index` stay valid.
        self.normalize_before_action();
        self.insert(section, index, &task)
    }

    fn done(&mut self) -> Result<()> {
        let Some(Row::Task(s, t)) = self.selected() else {
            return Ok(());
        };
        let section = &self.sections[s];
        let task = &section.tasks[t];

        let date = jiff::Zoned::now().date().to_string();
        let archive_dir = self.root.join("archive");
        let archive_path = archive_dir.join(format!("{date}.md"));
        // Not an op: an empty `archive/` left behind by undo does no harm.
        fs::create_dir_all(&archive_dir)
            .wrap_err_with(|| format!("failed to create {}", archive_dir.display()))?;
        let existing = read_if_exists(&archive_path)?;

        let ops = edit::archive(task, section, &archive_path, existing.as_deref(), &date);
        self.apply(ops)?;
        self.clamp_cursor();
        Ok(())
    }

    /// `J` / `K`: moves the task or section under the cursor one place, the cursor follows.
    /// Nothing happens at the edges.
    fn move_selected(&mut self, down: bool) -> Result<()> {
        let ops = match self.selected() {
            Some(Row::Task(s, t)) => {
                let Some((to, index)) = self.task_target(s, t, down) else {
                    return Ok(());
                };
                edit::move_task(&self.sections, s, t, to, index)
                    .ok_or_else(|| eyre!("section is full (999 tasks)"))?
            }
            Some(Row::Section(s)) => match edit::move_section(&self.sections, s, down) {
                Some(ops) => ops,
                None => return Ok(()),
            },
            None => return Ok(()),
        };
        // The moved entry is always the last rename.
        let moved_to = match ops.last() {
            Some(Op::Rename { to, .. }) => Some(to.clone()),
            _ => None,
        };
        self.apply(ops)?;
        if let Some(path) = moved_to {
            self.select_path(&path);
        }
        Ok(())
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

    fn cut(&mut self) -> Result<()> {
        let Some(Row::Task(s, t)) = self.selected() else {
            return Ok(());
        };
        let task = self.sections[s].tasks[t].clone();
        self.apply(edit::cut(&task))?;
        self.register = Some(task);
        // Cursor stays on the same row, which is now the next task (like vim).
        self.clamp_cursor();
        Ok(())
    }

    fn paste(&mut self, above: bool) -> Result<()> {
        let Some(task) = self.register.clone() else {
            return Err(eyre!("nothing to paste"));
        };
        let Some((s, index)) = self.insert_position(above) else {
            return Ok(());
        };
        self.insert(s, index, &task)
    }

    /// Section and index where `p` / `o` (below) or `P` / `O` (above) put a task.
    /// On a section header both put it first in that section.
    fn insert_position(&self, above: bool) -> Option<(usize, usize)> {
        match self.selected()? {
            Row::Task(s, t) if above => Some((s, t)),
            Row::Task(s, t) => Some((s, t + 1)),
            Row::Section(s) => Some((s, 0)),
        }
    }

    /// Writes `task` into section `s` at `index` and puts the cursor on it.
    fn insert(&mut self, s: usize, index: usize, task: &Task) -> Result<()> {
        let ops = edit::paste(&self.sections[s], index, task)
            .ok_or_else(|| eyre!("section is full (999 tasks)"))?;
        let created = created_path(&ops);
        self.apply(ops)?;
        if let Some(path) = created {
            self.select_path(&path);
        }
        Ok(())
    }

    fn undo(&mut self) -> Result<()> {
        let Some(ops) = self.undo.last() else {
            return Err(eyre!("nothing to undo"));
        };
        let inverse = op::inverse_all(ops);
        // Only pop once the inverse worked, so a failed undo can be retried.
        op::apply_all(&inverse)?;
        self.undo.pop();
        self.reload()?;
        self.clamp_cursor();
        // Undoing a cut brings the file back, put the cursor on it.
        if let Some(path) = created_path(&inverse) {
            self.select_path(&path);
        }
        Ok(())
    }

    /// Applies ops, records them for undo and reloads from disk.
    fn apply(&mut self, ops: Vec<Op>) -> Result<()> {
        op::apply_all(&ops)?;
        self.undo.push(ops);
        self.reload()
    }

    /// Reads `todo/` again. The disk is the source of truth, `sections` is only a view of it.
    fn reload(&mut self) -> Result<()> {
        self.sections = load::load(&self.root.join("todo"))?;
        Ok(())
    }

    fn clamp_cursor(&mut self) {
        self.cursor = self.cursor.min(self.rows().len().saturating_sub(1));
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
    /// Computed on demand instead of stored, so it can never be out of sync with `sections`.
    pub fn rows(&self) -> Vec<Row> {
        let mut rows = Vec::new();
        for (s, section) in self.sections.iter().enumerate() {
            rows.push(Row::Section(s));
            rows.extend((0..section.tasks.len()).map(|t| Row::Task(s, t)));
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

fn renamed_message(count: usize) -> String {
    if count == 1 {
        "renamed 1 file to match its title".to_string()
    } else {
        format!("renamed {count} files to match their titles")
    }
}

fn created_path(ops: &[Op]) -> Option<PathBuf> {
    ops.iter().find_map(|op| match op {
        Op::Create { path, .. } => Some(path.clone()),
        _ => None,
    })
}

#[cfg(test)]
pub mod tests {
    use std::fs;

    use ratatui::crossterm::event::KeyCode;
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
    /// Each file gets its slug as heading: `010-a.md` -> `# a`.
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
        let (_dir, mut app) = app_with_files(&["010-a.md"]);
        app.cursor = 1;
        app.update(Action::Help);
        assert!(app.show_help);

        app.handle_event(Event::Key(KeyCode::Char('x').into()));
        assert!(!app.show_help);
        assert_eq!(listing(&app), ["[inbox]", "010-a.md"], "x did not archive");
    }

    #[test]
    fn reload_keeps_cursor_on_the_same_task() {
        let (dir, mut app) = app_with_files(&["010-a.md", "020-b.md"]);
        app.cursor = 2;

        // Changes from outside: a task appears above, one disappears.
        let todo = dir.path().join("todo");
        fs::write(todo.join("005-z.md"), "# z").unwrap();
        fs::write(todo.join("003-y.md"), "# y").unwrap();
        fs::remove_file(todo.join("010-a.md")).unwrap();
        app.reload_keep_selection().unwrap();

        assert_eq!(
            listing(&app),
            ["[inbox]", "003-y.md", "005-z.md", "020-b.md"]
        );
        assert_eq!(app.cursor, 3, "still on 020-b.md");
    }

    #[test]
    fn reload_clamps_cursor_if_task_is_gone() {
        let (dir, mut app) = app_with_files(&["010-a.md"]);
        app.cursor = 1;
        fs::remove_file(dir.path().join("todo/010-a.md")).unwrap();
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
    fn cut_and_paste_into_other_section() {
        let (_dir, mut app) = app_with_files(&["010-a.md", "020-b.md", "010-work/"]);
        assert_eq!(listing(&app), ["[inbox]", "010-a.md", "020-b.md", "[work]"]);

        app.cursor = 1;
        app.update(Action::Cut);
        assert_eq!(listing(&app), ["[inbox]", "020-b.md", "[work]"]);
        assert_eq!(app.cursor, 1, "cursor moves onto the next task");

        app.cursor = 2;
        app.update(Action::PasteBelow);
        assert_eq!(app.status, None);
        assert_eq!(listing(&app), ["[inbox]", "020-b.md", "[work]", "010-a.md"]);
        assert_eq!(app.cursor, 3, "cursor is on the pasted task");
    }

    #[test]
    fn paste_above_and_below() {
        let (_dir, mut app) = app_with_files(&["010-a.md", "020-b.md", "030-c.md"]);
        app.cursor = 3;
        app.update(Action::Cut);

        app.cursor = 1;
        app.update(Action::PasteAbove);
        assert_eq!(listing(&app)[1], "005-c.md");

        app.cursor = 2;
        app.update(Action::PasteBelow);
        assert_eq!(listing(&app)[3], "015-c-2.md");
    }

    #[test]
    fn undo_reverts_paste_then_cut() {
        let (_dir, mut app) = app_with_files(&["010-a.md", "010-work/"]);
        let before = listing(&app);

        app.cursor = 1;
        app.update(Action::Cut);
        app.cursor = 1;
        app.update(Action::PasteBelow);

        app.update(Action::Undo);
        assert_eq!(listing(&app), ["[inbox]", "[work]"]);
        app.update(Action::Undo);
        assert_eq!(listing(&app), before);
        assert_eq!(app.cursor, 1, "cursor is on the restored task");

        app.update(Action::Undo);
        assert_eq!(
            app.status,
            Some(Status::Error("nothing to undo".to_string()))
        );
    }

    #[test]
    fn errors_go_to_status() {
        let (_dir, mut app) = app_with_files(&["010-a.md"]);
        app.update(Action::PasteBelow);
        assert_eq!(
            app.status,
            Some(Status::Error("nothing to paste".to_string()))
        );
    }

    #[test]
    fn open_requests_editor_for_task_only() {
        let (_dir, mut app) = app_with_files(&["010-a.md"]);
        app.update(Action::Open);
        assert_eq!(app.editor_request, None, "section header opens nothing");

        app.cursor = 1;
        app.update(Action::Open);
        let Some(EditorRequest::Open(path)) = app.editor_request.clone() else {
            panic!("expected an open request");
        };
        assert!(path.ends_with("todo/010-a.md"));
    }

    #[test]
    fn new_requests_create_at_insert_position() {
        let (_dir, mut app) = app_with_files(&["010-a.md", "020-b.md"]);
        app.cursor = 1;
        app.update(Action::NewBelow);
        assert_eq!(
            app.editor_request,
            Some(EditorRequest::Create {
                section: 0,
                index: 1
            })
        );
        app.update(Action::NewAbove);
        assert_eq!(
            app.editor_request,
            Some(EditorRequest::Create {
                section: 0,
                index: 0
            })
        );
    }

    #[test]
    fn after_create_inserts_and_undo_removes() {
        let (_dir, mut app) = app_with_files(&["010-a.md", "020-b.md"]);
        app.after_create(0, 1, "# New task\n\nnotes\n").unwrap();
        assert_eq!(
            listing(&app),
            ["[inbox]", "010-a.md", "015-new-task.md", "020-b.md"]
        );
        assert_eq!(app.cursor, 2, "cursor is on the new task");

        app.update(Action::Undo);
        assert_eq!(listing(&app), ["[inbox]", "010-a.md", "020-b.md"]);
    }

    #[test]
    fn after_create_aborts_without_title() {
        let (_dir, mut app) = app_with_files(&["010-a.md"]);
        for content in ["# \n", "", "text but no heading\n"] {
            let err = app.after_create(0, 1, content).unwrap_err();
            assert_eq!(err.to_string(), "aborted, empty title");
        }
        assert_eq!(listing(&app), ["[inbox]", "010-a.md"]);
    }

    #[test]
    fn after_edit_renames_to_new_heading() {
        let (_dir, mut app) = app_with_files(&["010-a.md", "020-b.md"]);
        app.cursor = 1;
        let path = app.sections[0].tasks[0].path.clone();

        // What the user does in the editor.
        fs::write(&path, "# Renamed task\n\nnotes").unwrap();
        app.after_edit().unwrap();

        assert_eq!(
            listing(&app),
            ["[inbox]", "010-renamed-task.md", "020-b.md"]
        );
        assert_eq!(app.cursor, 1, "cursor follows the renamed file");
        assert_eq!(app.sections[0].tasks[0].body, "notes");
    }

    #[test]
    fn startup_normalizes_and_lists_renames() {
        let (_dir, mut app) = app_with_files(&["010-a.md", "b.md", "010-work/old.md"]);
        assert_eq!(
            listing(&app),
            ["[inbox]", "010-a.md", "020-b.md", "[work]", "010-old.md"]
        );
        assert_eq!(
            app.renamed,
            [
                ("b.md".into(), "020-b.md".into()),
                ("010-work/old.md".into(), "010-work/010-old.md".into()),
            ]
        );

        // Any key closes the popup and does nothing else.
        app.handle_event(Event::Key(KeyCode::Char('j').into()));
        assert!(app.renamed.is_empty());
        assert_eq!(app.cursor, 0);
    }

    #[test]
    fn reload_from_watcher_does_not_rename() {
        let (dir, mut app) = app_with_files(&["010-a.md"]);
        fs::write(dir.path().join("todo/new.md"), "# New").unwrap();
        app.reload_keep_selection().unwrap();
        assert_eq!(listing(&app), ["[inbox]", "010-a.md", "new.md"]);
    }

    #[test]
    fn action_normalizes_first_and_says_so() {
        let (dir, mut app) = app_with_files(&["010-a.md", "020-b.md"]);
        fs::write(dir.path().join("todo/new.md"), "# New").unwrap();
        app.reload_keep_selection().unwrap();
        app.cursor = 1;

        app.update(Action::Cut);
        assert_eq!(listing(&app), ["[inbox]", "020-b.md", "030-new.md"]);
        assert_eq!(
            app.status,
            Some(Status::Info(
                "renamed 1 file to match its title".to_string()
            ))
        );

        // Undo only reverts the cut, the rename stays.
        app.update(Action::Undo);
        assert_eq!(
            listing(&app),
            ["[inbox]", "010-a.md", "020-b.md", "030-new.md"]
        );
    }

    #[test]
    fn move_task_within_and_across_sections() {
        let (_dir, mut app) = app_with_files(&["010-a.md", "020-b.md", "010-work/010-c.md"]);
        app.cursor = 1;

        app.update(Action::MoveDown);
        assert_eq!(
            listing(&app),
            ["[inbox]", "020-b.md", "030-a.md", "[work]", "010-c.md"]
        );
        assert_eq!(app.cursor, 2, "cursor follows the task");

        app.update(Action::MoveDown);
        assert_eq!(
            listing(&app),
            ["[inbox]", "020-b.md", "[work]", "005-a.md", "010-c.md"]
        );
        assert_eq!(app.cursor, 3);

        // One undo per move.
        app.update(Action::Undo);
        assert_eq!(
            listing(&app),
            ["[inbox]", "020-b.md", "030-a.md", "[work]", "010-c.md"]
        );
    }

    #[test]
    fn move_task_stops_at_the_ends() {
        let (_dir, mut app) = app_with_files(&["010-a.md", "010-work/010-b.md"]);
        app.cursor = 1;
        app.update(Action::MoveUp);
        app.cursor = 3;
        app.update(Action::MoveDown);
        assert_eq!(listing(&app), ["[inbox]", "010-a.md", "[work]", "010-b.md"]);
        assert_eq!(app.status, None);
    }

    #[test]
    fn move_section_with_its_tasks() {
        let (_dir, mut app) = app_with_files(&["010-work/010-a.md", "020-home/010-b.md", "030-x/"]);
        app.cursor = 1;

        app.update(Action::MoveDown);
        assert_eq!(
            listing(&app),
            ["[inbox]", "[home]", "010-b.md", "[work]", "010-a.md", "[x]"]
        );
        assert_eq!(app.cursor, 3, "cursor follows the section");
        assert!(app.sections[2].dir.ends_with("025-work"));

        // The inbox never moves.
        app.cursor = 0;
        app.update(Action::MoveDown);
        assert_eq!(app.sections[1].title(), "home");
    }

    #[test]
    fn done_archives_and_undo_restores() {
        let (dir, mut app) = app_with_files(&["010-a.md", "010-work/010-b.md"]);
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
        assert_eq!(listing(&app), ["[inbox]", "[work]", "010-b.md"]);
        assert_eq!(
            fs::read_to_string(&archive_path).unwrap(),
            format!("# {today}\n\n## inbox\n\n### a\n")
        );

        app.update(Action::Undo);
        assert_eq!(listing(&app), ["[inbox]", "010-a.md", "[work]", "010-b.md"]);
        assert!(!archive_path.exists());
    }

    #[test]
    fn paste_renumbers_and_undo_reverts_it() {
        let (_dir, mut app) = app_with_files(&["010-a.md", "011-b.md", "012-c.md"]);
        let before = listing(&app);

        app.cursor = 3;
        app.update(Action::Cut);
        app.cursor = 1;
        app.update(Action::PasteBelow);
        assert_eq!(app.status, None);
        assert_eq!(
            listing(&app),
            ["[inbox]", "010-a.md", "020-c.md", "030-b.md"]
        );
        assert_eq!(app.cursor, 2, "cursor is on the pasted task");

        // One undo reverts the paste including the renumbering, the next one the cut.
        app.update(Action::Undo);
        assert_eq!(listing(&app), ["[inbox]", "010-a.md", "011-b.md"]);
        app.update(Action::Undo);
        assert_eq!(listing(&app), before);
    }
}
