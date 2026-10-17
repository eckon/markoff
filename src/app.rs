//! App state, the run loop, and what each [`Action`] does to it and to the disk.

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
    daily::{self, DailyTask},
    edit, editor, filter,
    keymap::{self, Action, Keymap},
    load,
    op::{self, Op},
    order::{self, OrderFile},
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
    /// The `daily` header, right after the inbox, see [`daily`].
    Daily,
    /// Task index in `daily.md`.
    DailyTask(usize),
}

/// Work that needs the editor. Set by `update`, handled by the run loop, which owns
/// the terminal. Keeps `update` testable without a terminal.
#[derive(Debug, Clone, PartialEq, Eq)]
enum EditorRequest {
    /// `<CR>`: edit an existing task.
    Open(PathBuf),
    /// `n`: write a new task, inserted at `index` in section `section`.
    Create { section: usize, index: usize },
    /// `N`: write a new section, inserted after section `section`.
    CreateSection(usize),
    /// `<CR>` on a section header: rename section `section`.
    RenameSection(usize),
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
    /// `todo/.order` as read with `sections`, the base for the order an action writes.
    order_file: OrderFile,
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
    /// The tasks of `daily.md`, `None` if there is no such file.
    pub daily: Option<Vec<DailyTask>>,
}

impl App {
    /// Without an order file, [`App::load`] reads the real one.
    pub fn new(root: PathBuf, sections: Vec<Section>) -> Self {
        let order_file = OrderFile {
            path: root.join("todo").join(order::FILE),
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
            renamed: Vec::new(),
            editor_request: None,
            filter: String::new(),
            filter_input: false,
            list_offset: 0,
            daily: None,
        }
    }

    /// Loads `todo/` and normalizes it. Renames are listed in a popup, since files changing
    /// their names on start can be a surprise.
    pub fn load(root: PathBuf) -> Result<Self> {
        let todo_dir = root.join("todo");
        let (sections, order_file) = load::load(&todo_dir)?;
        let mut app = Self::new(root, sections);
        app.order_file = order_file;
        app.daily = load_daily(&app.root)?;
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
                        editor::create(terminal, &self.sections[section].dir, "# \n")
                            .and_then(|content| self.after_create(section, index, &content))
                    }
                    EditorRequest::CreateSection(section) => {
                        let draft = edit::section_draft("");
                        editor::create(terminal, &self.sections[0].dir, &draft)
                            .and_then(|content| self.after_create_section(section, &content))
                    }
                    EditorRequest::RenameSection(section) => {
                        let draft = edit::section_draft(&self.sections[section].title());
                        editor::rename(terminal, &self.sections[0].dir, &draft)
                            .and_then(|content| self.after_rename_section(section, &content))
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
    ///
    /// Daily tasks share `daily.md`, so they get `daily.md#<index>`. Never touches the disk,
    /// only keeps the cursor on the right row, e.g. after undo.
    fn row_path(&self, row: Row) -> PathBuf {
        let daily_path = || self.root.join(daily::FILE);
        match row {
            Row::Section(s) => self.sections[s].dir.clone(),
            Row::Task(s, t) => self.sections[s].tasks[t].path.clone(),
            Row::Daily => daily_path(),
            Row::DailyTask(i) => PathBuf::from(format!("{}#{i}", daily_path().display())),
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
        // Where a task goes is unclear while tasks in between are hidden, and a new task or
        // section would mostly be hidden right away.
        if !self.filter.is_empty()
            && matches!(
                action,
                Action::MoveDown | Action::MoveUp | Action::New | Action::NewSection
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
            Action::MoveDown | Action::MoveUp => self.move_selected(action == Action::MoveDown)?,
            Action::Open => match self.selected() {
                Some(Row::Task(s, t)) => {
                    let path = self.sections[s].tasks[t].path.clone();
                    self.editor_request = Some(EditorRequest::Open(path));
                }
                Some(Row::Section(0)) => return Err(eyre!("the inbox can't be renamed")),
                Some(Row::Section(s)) => {
                    self.editor_request = Some(EditorRequest::RenameSection(s))
                }
                // `daily.md` is edited by hand.
                Some(Row::Daily | Row::DailyTask(_)) | None => {}
            },
            Action::New => {
                // At the end of the cursor's section, from its header or any of its tasks.
                if let Some(Row::Section(section) | Row::Task(section, _)) = self.selected() {
                    let index = self.sections[section].tasks.len();
                    self.editor_request = Some(EditorRequest::Create { section, index });
                }
            }
            Action::NewSection => {
                if let Some(Row::Section(section) | Row::Task(section, _)) = self.selected() {
                    self.editor_request = Some(EditorRequest::CreateSection(section));
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
        // Normalizing keeps the order of tasks and sections, so `section` and `index` stay valid.
        self.normalize_before_action();
        // `sections` is from before the editor. If the files changed meanwhile, the ops refuse
        // and the text is lost, so say that clearly.
        self.insert(section, index, &task)
            .map_err(|err| eyre!("task not added: {err}"))
    }

    /// Creates a section named by the heading written in the editor, after section `s`.
    fn after_create_section(&mut self, s: usize, content: &str) -> Result<()> {
        let (slug, more) =
            edit::section_name(content).ok_or_else(|| eyre!("aborted, empty title"))?;
        self.ensure_no_section(&slug)?;
        // Normalizing keeps the order of sections, so `s` stays valid.
        self.normalize_before_action();
        let (ops, created) = edit::insert_section(&self.sections, s, &slug, &self.order_file);
        self.apply(ops)?;
        self.select_path(&created);
        if more {
            self.status = Some(Status::Info(
                "created, only the heading is used for a section".to_string(),
            ));
        }
        Ok(())
    }

    /// Two sections with the same name would look the same in the list and the archive.
    fn ensure_no_section(&self, slug: &str) -> Result<()> {
        if self.sections.iter().any(|section| section.title() == slug) {
            return Err(eyre!("a section named {slug} already exists"));
        }
        Ok(())
    }

    /// Renames section `s` to the heading written in the editor.
    fn after_rename_section(&mut self, s: usize, content: &str) -> Result<()> {
        let (slug, more) =
            edit::section_name(content).ok_or_else(|| eyre!("aborted, empty title"))?;
        if slug == self.sections[s].title() {
            return Ok(());
        }
        self.ensure_no_section(&slug)?;
        // Normalizing keeps the order of sections, so `s` stays valid.
        self.normalize_before_action();
        let (ops, to) = edit::rename_section(&self.sections, s, &slug, &self.order_file);
        self.apply(ops)?;
        self.select_path(&to);
        if more {
            self.status = Some(Status::Info(
                "renamed, only the heading is used for a section".to_string(),
            ));
        }
        Ok(())
    }

    fn done(&mut self) -> Result<()> {
        let (s, t) = match self.selected() {
            Some(Row::Task(s, t)) => (s, t),
            Some(Row::DailyTask(i)) => return self.toggle_daily(i),
            _ => return Ok(()),
        };
        let date = today();
        let archive_dir = self.root.join("archive");
        let archive_path = archive_dir.join(format!("{date}.md"));
        // Not an op: an empty `archive/` left behind by undo does no harm.
        fs::create_dir_all(&archive_dir)
            .wrap_err_with(|| format!("failed to create {}", archive_dir.display()))?;
        let existing = read_if_exists(&archive_path)?;

        let ops = edit::archive(
            &self.sections,
            s,
            t,
            &archive_path,
            existing.as_deref(),
            &date,
            &self.order_file,
        );
        self.apply(ops)
    }

    /// `X` on a daily task: done for today, or open again. The task stays in the list.
    fn toggle_daily(&mut self, i: usize) -> Result<()> {
        let path = self.root.join(daily::FILE);
        let before = fs::read_to_string(&path)
            .wrap_err_with(|| format!("failed to read {}", path.display()))?;
        let after = daily::toggle(&before, i, &today());
        self.apply(vec![Op::Update {
            path,
            before,
            after,
        }])
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

    /// `}`: the next section header below the cursor, the last row if there is none (like vim).
    fn next_section_row(&self) -> usize {
        let rows = self.rows();
        (self.cursor + 1..rows.len())
            .find(|&i| matches!(rows[i], Row::Section(_) | Row::Daily))
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
            Some(Row::Daily | Row::DailyTask(_)) | None => return Ok(()),
        };
        let title = self.sections[s].tasks[t].title();
        // Cursor stays on the same row, which is now the next task (like vim).
        let ops = edit::delete(&self.sections, s, t, &self.order_file);
        self.apply(ops)?;
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
        let ops = edit::delete_section(&self.sections, s, &self.order_file);
        self.apply(ops)?;
        self.status = Some(Status::Info(format!("deleted {title} (u to undo)")));
        Ok(())
    }

    /// Writes `task` into section `s` at `index` and puts the cursor on it.
    fn insert(&mut self, s: usize, index: usize, task: &Task) -> Result<()> {
        let (ops, created) = edit::insert(&self.sections, s, index, task, &self.order_file);
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
        (self.sections, self.order_file) = load::load(&self.root.join("todo"))?;
        self.daily = load_daily(&self.root)?;
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
    /// The daily checklist comes right after the inbox, hidden while a filter is set.
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
            if s == 0
                && self.filter.is_empty()
                && let Some(daily) = &self.daily
            {
                rows.push(Row::Daily);
                rows.extend((0..daily.len()).map(Row::DailyTask));
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
/// `YYYY-MM-DD`, for the archive file and the daily marker.
pub fn today() -> String {
    jiff::Zoned::now().date().to_string()
}

fn load_daily(root: &Path) -> Result<Option<Vec<DailyTask>>> {
    Ok(read_if_exists(&root.join(daily::FILE))?.map(|content| daily::parse(&content)))
}

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

#[cfg(test)]
pub mod tests {
    use std::fs;

    use ratatui::crossterm::event::{KeyCode, KeyEvent};
    use tempfile::TempDir;

    use super::*;
    use crate::order::{Group, Order};

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
    /// Each file gets its name as heading: `a.md` -> `# a`. The order file lists everything
    /// in the given order.
    fn app_with_files(files: &[&str]) -> (TempDir, App) {
        let dir = TempDir::new().unwrap();
        let todo = dir.path().join("todo");
        fs::create_dir(&todo).unwrap();
        let mut order = Order::default();
        for file in files {
            let (section, name) = file.rsplit_once('/').unwrap_or(("", file));
            if !section.is_empty() && order.section_position(section).is_none() {
                order.sections.push(Group {
                    name: section.to_string(),
                    tasks: Vec::new(),
                });
            }
            let path = todo.join(file);
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
                .rfind(|group| group.name == section);
            group.unwrap().tasks.push(stem.to_string());
        }
        fs::write(todo.join(order::FILE), order.format()).unwrap();
        let app = App::load(dir.path().to_path_buf()).unwrap();
        (dir, app)
    }

    fn order_file(dir: &TempDir) -> String {
        fs::read_to_string(dir.path().join("todo").join(order::FILE)).unwrap()
    }

    /// File names on disk in row order, sections as `[name]`.
    fn listing(app: &App) -> Vec<String> {
        app.rows()
            .into_iter()
            .map(|row| match row {
                Row::Section(s) => format!("[{}]", app.sections[s].title()),
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
        let (_dir, mut app) = app_with_files(&["a.md"]);
        app.cursor = 1;
        app.update(Action::Help);
        assert!(app.show_help);

        app.handle_event(Event::Key(KeyCode::Char('x').into()));
        assert!(!app.show_help);
        assert_eq!(listing(&app), ["[inbox]", "a.md"], "x did not archive");
    }

    #[test]
    fn reload_keeps_cursor_on_the_same_task() {
        let (dir, mut app) = app_with_files(&["a.md", "b.md"]);
        app.cursor = 2;

        // Changes from outside (a git pull): tasks appear above, one disappears.
        let todo = dir.path().join("todo");
        fs::write(todo.join("z.md"), "# z").unwrap();
        fs::write(todo.join("y.md"), "# y").unwrap();
        fs::remove_file(todo.join("a.md")).unwrap();
        fs::write(todo.join(order::FILE), "y\nz\nb\n").unwrap();
        app.reload_keep_selection().unwrap();

        assert_eq!(listing(&app), ["[inbox]", "y.md", "z.md", "b.md"]);
        assert_eq!(app.cursor, 3, "still on b.md");
    }

    #[test]
    fn reload_clamps_cursor_if_task_is_gone() {
        let (dir, mut app) = app_with_files(&["a.md"]);
        app.cursor = 1;
        fs::remove_file(dir.path().join("todo/a.md")).unwrap();
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
        let (dir, mut app) = app_with_files(&["a.md", "b.md"]);
        app.cursor = 1;

        app.update(Action::Delete);
        assert_eq!(listing(&app), ["[inbox]", "b.md"]);
        assert_eq!(order_file(&dir), "b\n");
        assert_eq!(app.cursor, 1, "cursor moves onto the next task");
        assert_eq!(
            app.status,
            Some(Status::Info("deleted a (u to undo)".to_string()))
        );

        app.update(Action::Undo);
        assert_eq!(listing(&app), ["[inbox]", "a.md", "b.md"]);
        assert_eq!(order_file(&dir), "a\nb\n");
        assert_eq!(app.cursor, 1, "cursor is on the restored task");

        app.update(Action::Undo);
        assert_eq!(
            app.status,
            Some(Status::Error("nothing to undo".to_string()))
        );
    }

    /// `app_with_files` plus a `daily.md` with `content`.
    fn app_with_daily(files: &[&str], content: &str) -> (TempDir, App) {
        let (dir, mut app) = app_with_files(files);
        fs::write(dir.path().join(daily::FILE), content).unwrap();
        app.reload(None).unwrap();
        (dir, app)
    }

    #[test]
    fn daily_rows_after_inbox_hidden_by_filter() {
        let (_dir, mut app) = app_with_daily(&["a.md", "work/b.md"], "## Jira\n## Menu\n");
        assert_eq!(
            app.rows(),
            [
                Row::Section(0),
                Row::Task(0, 0),
                Row::Daily,
                Row::DailyTask(0),
                Row::DailyTask(1),
                Row::Section(1),
                Row::Task(1, 0),
            ]
        );
        app.update(Action::NextSection);
        assert_eq!(
            app.selected(),
            Some(Row::Daily),
            "`}}` stops at the daily header"
        );

        // Even when a daily task would match.
        type_filter(&mut app, "jira");
        assert!(!app.rows().contains(&Row::Daily));
    }

    #[test]
    fn daily_done_toggles_and_undo_restores() {
        let content = "# Daily\n\n## Jira\n## Menu\n";
        let (dir, mut app) = app_with_daily(&[], content);
        let path = dir.path().join(daily::FILE);
        app.cursor = 3;

        app.update(Action::Done);
        let today = today();
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            format!("# Daily\n\n## Jira\n## Menu [done {today}]\n")
        );
        assert_eq!(app.selected(), Some(Row::DailyTask(1)), "stays in the list");
        assert!(app.daily.as_ref().unwrap()[1].done_on(&today));

        app.update(Action::Done);
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            content,
            "X again opens it"
        );

        app.update(Action::Undo);
        app.cursor = 0;
        app.update(Action::Undo);
        assert_eq!(fs::read_to_string(&path).unwrap(), content);
        assert_eq!(
            app.selected(),
            Some(Row::DailyTask(1)),
            "undo goes back to the task"
        );
    }

    #[test]
    fn daily_rows_ignore_other_keys() {
        let content = "## Jira\n";
        let (dir, mut app) = app_with_daily(&["a.md"], content);
        for cursor in [2, 3] {
            app.cursor = cursor;
            for action in [
                Action::MoveDown,
                Action::MoveUp,
                Action::Delete,
                Action::Open,
                Action::New,
                Action::NewSection,
            ] {
                app.update(action);
                assert_eq!(app.status, None, "{action:?}");
                assert_eq!(app.editor_request, None, "{action:?}");
            }
        }
        assert_eq!(
            fs::read_to_string(dir.path().join(daily::FILE)).unwrap(),
            content
        );
        assert_eq!(listing(&app), ["[inbox]", "a.md", "[daily]", "Jira"]);
    }

    #[test]
    fn delete_section_only_if_empty_and_not_inbox() {
        let (dir, mut app) = app_with_files(&["a.md", "work/", "home/b.md"]);
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
        assert_eq!(listing(&app), ["[inbox]", "a.md", "[home]", "b.md"]);
        assert!(!dir.path().join("todo/work").exists());
        assert_eq!(order_file(&dir), "a\n\n# home\nb\n");
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
            "fix-login.md",
            "buy-milk.md",
            "work/review.md",
            "home/garden.md",
        ]);
        fs::write(
            dir.path().join("todo/home/garden.md"),
            "# garden\n\nFIX the fence",
        )
        .unwrap();
        app.reload(None).unwrap();

        // Title and body, ignoring case. Sections without a match are hidden.
        type_filter(&mut app, "fix");
        assert_eq!(
            listing(&app),
            ["[inbox]", "fix-login.md", "[home]", "garden.md"]
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
        let (_dir, mut app) = app_with_files(&["a-fix.md", "b.md", "c-fix.md"]);
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
        let (_dir, mut app) = app_with_files(&["a.md", "b.md"]);
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
        let (_dir, mut app) = app_with_files(&["a.md", "b.md"]);
        type_filter(&mut app, "a");
        app.update(Action::FilterAccept);
        app.cursor = 1;
        for action in [
            Action::MoveDown,
            Action::MoveUp,
            Action::New,
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
        assert_eq!(listing(&app), ["[inbox]", "a.md", "b.md"]);
    }

    #[test]
    fn hint_goes_to_status() {
        let (_dir, mut app) = app_with_files(&[]);
        app.update(Action::Hint("use X"));
        assert_eq!(app.status, Some(Status::Info("use X".to_string())));
    }

    #[test]
    fn open_requests_editor_for_task_and_section() {
        let (_dir, mut app) = app_with_files(&["a.md", "work/"]);
        app.update(Action::Open);
        assert_eq!(app.editor_request, None);
        assert_eq!(
            app.status,
            Some(Status::Error("the inbox can't be renamed".to_string()))
        );

        app.cursor = 2;
        app.update(Action::Open);
        assert_eq!(app.editor_request, Some(EditorRequest::RenameSection(1)));

        app.cursor = 1;
        app.update(Action::Open);
        let Some(EditorRequest::Open(path)) = app.editor_request.clone() else {
            panic!("expected an open request");
        };
        assert!(path.ends_with("todo/a.md"));
    }

    #[test]
    fn new_requests_create_at_section_end() {
        let (_dir, mut app) = app_with_files(&["a.md", "b.md", "work/c.md"]);
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
        let (dir, mut app) = app_with_files(&["a.md", "b.md"]);
        // `n` was pressed on `a`.
        app.cursor = 1;
        app.after_create(0, 2, "# New task\n\nnotes\n").unwrap();
        assert_eq!(listing(&app), ["[inbox]", "a.md", "b.md", "new-task.md"]);
        assert_eq!(order_file(&dir), "a\nb\nnew-task\n");
        assert_eq!(app.cursor, 3, "cursor is on the new task");

        app.update(Action::Undo);
        assert_eq!(listing(&app), ["[inbox]", "a.md", "b.md"]);
        assert_eq!(
            app.cursor, 1,
            "cursor is back on `a`, where `n` was pressed"
        );
    }

    #[test]
    fn insert_at_the_top_touches_no_other_file() {
        let (dir, mut app) = app_with_files(&["a.md", "b.md", "c.md"]);
        app.cursor = 1;
        app.after_create(0, 0, "# New").unwrap();
        assert_eq!(listing(&app), ["[inbox]", "new.md", "a.md", "b.md", "c.md"]);
        assert_eq!(order_file(&dir), "new\na\nb\nc\n");
        assert_eq!(app.cursor, 1, "cursor is on the new task");

        // One undo reverts the file and the order.
        app.update(Action::Undo);
        assert_eq!(listing(&app), ["[inbox]", "a.md", "b.md", "c.md"]);
        assert_eq!(order_file(&dir), "a\nb\nc\n");
    }

    #[test]
    fn after_create_aborts_without_title() {
        let (_dir, mut app) = app_with_files(&["a.md"]);
        for content in ["# \n", "", "text but no heading\n"] {
            let err = app.after_create(0, 1, content).unwrap_err();
            assert_eq!(err.to_string(), "aborted, empty title");
        }
        assert_eq!(listing(&app), ["[inbox]", "a.md"]);
    }

    #[test]
    fn after_create_fails_on_changes_made_while_editing() {
        let (dir, mut app) = app_with_files(&["a.md", "b.md"]);
        app.cursor = 1;
        // A sync reorders while the editor is open. That order must not be overwritten.
        let todo = dir.path().join("todo");
        fs::write(todo.join(order::FILE), "b\na\n").unwrap();

        let err = app.after_create(0, 1, "# New").unwrap_err();
        assert!(err.to_string().starts_with("task not added: "), "{err}");
        assert!(!todo.join("new.md").exists(), "nothing written");
        assert_eq!(order_file(&dir), "b\na\n");
    }

    #[test]
    fn after_create_section_inserts_and_undo_removes() {
        let (dir, mut app) = app_with_files(&["a.md", "work/b.md", "home/"]);
        let before = listing(&app);
        let draft = edit::section_draft;

        assert_eq!(
            app.after_create_section(1, &draft(""))
                .unwrap_err()
                .to_string(),
            "aborted, empty title"
        );
        assert_eq!(
            app.after_create_section(1, &draft("Home"))
                .unwrap_err()
                .to_string(),
            "a section named home already exists"
        );
        assert_eq!(listing(&app), before);

        // `N` was pressed on `b`: after `work`, before `home`.
        app.cursor = 3;
        let content = format!("{}\nnotes", draft("Garden"));
        app.after_create_section(1, &content).unwrap();
        assert_eq!(
            listing(&app),
            ["[inbox]", "a.md", "[work]", "b.md", "[garden]", "[home]"]
        );
        assert!(dir.path().join("todo/garden").is_dir());
        assert_eq!(order_file(&dir), "a\n\n# work\nb\n\n# garden\n\n# home\n");
        assert_eq!(app.cursor, 4, "cursor on the new section");
        assert_eq!(
            app.status,
            Some(Status::Info(
                "created, only the heading is used for a section".to_string()
            ))
        );

        app.update(Action::Undo);
        assert_eq!(listing(&app), before);
        assert!(!dir.path().join("todo/garden").exists());
        assert_eq!(app.cursor, 3, "back on `b`");
    }

    #[test]
    fn new_section_requests_create_after_cursor_section() {
        let (_dir, mut app) = app_with_files(&["a.md", "work/b.md"]);
        for (cursor, section) in [(0, 0), (1, 0), (2, 1), (3, 1)] {
            app.cursor = cursor;
            app.update(Action::NewSection);
            assert_eq!(
                app.editor_request,
                Some(EditorRequest::CreateSection(section)),
                "cursor {cursor}"
            );
        }
    }

    #[test]
    fn after_rename_section_renames_and_undo_reverts() {
        let (dir, mut app) = app_with_files(&["work/a.md", "home/"]);
        let before = listing(&app);
        let template = edit::section_draft("work");
        let draft = |title: &str| template.replace("# work", title);

        // Empty title, unchanged name and a clash change nothing.
        let (empty, same, clash) = (draft("# "), draft("# work"), draft("# Home"));
        assert_eq!(
            app.after_rename_section(1, &empty).unwrap_err().to_string(),
            "aborted, empty title"
        );
        app.after_rename_section(1, &same).unwrap();
        assert_eq!(
            app.after_rename_section(1, &clash).unwrap_err().to_string(),
            "a section named home already exists"
        );
        assert_eq!(listing(&app), before);
        assert!(app.undo.is_empty());

        // Text below the heading is dropped, the status says so.
        app.cursor = 2;
        let renamed = format!("{}\nnotes", draft("# Work Stuff"));
        app.after_rename_section(1, &renamed).unwrap();
        assert!(dir.path().join("todo/work-stuff/a.md").exists());
        assert_eq!(order_file(&dir), "# work-stuff\na\n\n# home\n");
        assert_eq!(
            app.selected(),
            Some(Row::Section(1)),
            "cursor on the section"
        );
        assert_eq!(
            app.status,
            Some(Status::Info(
                "renamed, only the heading is used for a section".to_string()
            ))
        );

        app.update(Action::Undo);
        assert_eq!(listing(&app), before);
        assert_eq!(app.cursor, 2, "back on the task");
    }

    #[test]
    fn after_edit_renames_to_new_heading() {
        let (dir, mut app) = app_with_files(&["a.md", "b.md"]);
        app.cursor = 1;
        let path = app.sections[0].tasks[0].path.clone();

        // What the user does in the editor.
        fs::write(&path, "# Renamed task\n\nnotes").unwrap();
        app.after_edit().unwrap();

        assert_eq!(listing(&app), ["[inbox]", "renamed-task.md", "b.md"]);
        assert_eq!(order_file(&dir), "renamed-task\nb\n", "keeps its place");
        assert_eq!(app.cursor, 1, "cursor follows the renamed file");
        assert_eq!(app.sections[0].tasks[0].body, "notes");
    }

    #[test]
    fn startup_normalizes_and_lists_renames() {
        // Old numbered names, no order file yet.
        let dir = TempDir::new().unwrap();
        let todo = dir.path().join("todo");
        fs::create_dir_all(todo.join("01-work")).unwrap();
        fs::write(todo.join("02-b.md"), "# b").unwrap();
        fs::write(todo.join("01-a.md"), "# a").unwrap();
        fs::write(todo.join("01-work/old.md"), "# Old task").unwrap();
        let mut app = App::load(dir.path().to_path_buf()).unwrap();

        assert_eq!(
            listing(&app),
            ["[inbox]", "a.md", "b.md", "[01-work]", "old-task.md"]
        );
        assert_eq!(
            app.renamed,
            [
                ("01-a.md".into(), "a.md".into()),
                ("02-b.md".into(), "b.md".into()),
                ("01-work/old.md".into(), "01-work/old-task.md".into()),
            ]
        );
        assert_eq!(order_file(&dir), "a\nb\n\n# 01-work\nold-task\n");

        // Any key closes the popup and does nothing else.
        app.handle_event(Event::Key(KeyCode::Char('j').into()));
        assert!(app.renamed.is_empty());
        assert_eq!(app.cursor, 0);
    }

    #[test]
    fn startup_cleans_the_order_file() {
        let (dir, _) = app_with_files(&["a.md", "b.md", "work/", "home/"]);
        fs::write(
            dir.path().join("todo").join(order::FILE),
            "gone\nb\n# work\n",
        )
        .unwrap();

        // Not listed: to the end, by name. Listed but gone: dropped. No rename, no popup.
        let app = App::load(dir.path().to_path_buf()).unwrap();
        assert_eq!(
            listing(&app),
            ["[inbox]", "b.md", "a.md", "[work]", "[home]"]
        );
        assert_eq!(order_file(&dir), "b\na\n\n# work\n\n# home\n");
        assert!(app.renamed.is_empty());
    }

    #[test]
    fn reload_from_watcher_does_not_write() {
        let (dir, mut app) = app_with_files(&["a.md"]);
        fs::write(dir.path().join("todo/draft.md"), "# New").unwrap();
        app.reload_keep_selection().unwrap();
        assert_eq!(listing(&app), ["[inbox]", "a.md", "draft.md"]);
        assert_eq!(order_file(&dir), "a\n");
    }

    #[test]
    fn action_normalizes_first_and_says_so() {
        let (dir, mut app) = app_with_files(&["a.md", "b.md"]);
        fs::write(dir.path().join("todo/draft.md"), "# New").unwrap();
        app.reload_keep_selection().unwrap();
        app.cursor = 1;

        app.update(Action::MoveDown);
        assert_eq!(listing(&app), ["[inbox]", "b.md", "a.md", "new.md"]);
        assert_eq!(
            app.status,
            Some(Status::Info(
                "renamed 1 file to match its title".to_string()
            ))
        );

        // Undo only reverts the move, the rename stays.
        app.update(Action::Undo);
        assert_eq!(listing(&app), ["[inbox]", "a.md", "b.md", "new.md"]);
    }

    #[test]
    fn move_task_within_and_across_sections() {
        let (dir, mut app) = app_with_files(&["a.md", "b.md", "work/c.md"]);
        app.cursor = 1;

        app.update(Action::MoveDown);
        assert_eq!(listing(&app), ["[inbox]", "b.md", "a.md", "[work]", "c.md"]);
        assert_eq!(order_file(&dir), "b\na\n\n# work\nc\n");
        assert_eq!(app.cursor, 2, "cursor follows the task");

        app.update(Action::MoveDown);
        assert_eq!(listing(&app), ["[inbox]", "b.md", "[work]", "a.md", "c.md"]);
        assert!(dir.path().join("todo/work/a.md").exists());
        assert_eq!(app.cursor, 3);

        // One undo per move, the cursor goes back with the task.
        app.update(Action::Undo);
        assert_eq!(listing(&app), ["[inbox]", "b.md", "a.md", "[work]", "c.md"]);
        assert_eq!(app.cursor, 2);
        app.update(Action::Undo);
        assert_eq!(app.cursor, 1);
        assert_eq!(listing(&app)[1], "a.md");
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
        let (_dir, mut app) = app_with_files(&["a.md", "work/b.md"]);
        app.cursor = 1;
        app.update(Action::MoveUp);
        app.cursor = 3;
        app.update(Action::MoveDown);
        assert_eq!(listing(&app), ["[inbox]", "a.md", "[work]", "b.md"]);
        assert_eq!(app.status, None);
    }

    #[test]
    fn move_section_with_its_tasks() {
        let (dir, mut app) = app_with_files(&["work/a.md", "home/b.md", "x/"]);
        app.cursor = 1;

        app.update(Action::MoveDown);
        assert_eq!(
            listing(&app),
            ["[inbox]", "[home]", "b.md", "[work]", "a.md", "[x]"]
        );
        assert_eq!(app.cursor, 3, "cursor follows the section");
        assert_eq!(order_file(&dir), "# home\nb\n\n# work\na\n\n# x\n");

        app.update(Action::Undo);
        assert_eq!(app.sections[1].title(), "work");
        assert_eq!(app.cursor, 1, "cursor goes back with the section");

        // The inbox never moves.
        app.cursor = 0;
        app.update(Action::MoveDown);
        assert_eq!(app.sections[1].title(), "work");
    }

    #[test]
    fn done_archives_and_undo_restores() {
        let (dir, mut app) = app_with_files(&["a.md", "work/b.md"]);
        let today = jiff::Zoned::now().date().to_string();
        let archive_path = dir.path().join("archive").join(format!("{today}.md"));

        app.cursor = 1;
        app.update(Action::Done);
        app.cursor = 2;
        app.update(Action::Done);
        assert_eq!(app.status, None);
        assert_eq!(listing(&app), ["[inbox]", "[work]"]);
        assert_eq!(order_file(&dir), "# work\n");
        assert_eq!(
            fs::read_to_string(&archive_path).unwrap(),
            format!("# {today}\n\n## inbox\n\n### a\n\n## work\n\n### b\n")
        );

        app.update(Action::Undo);
        assert_eq!(listing(&app), ["[inbox]", "[work]", "b.md"]);
        assert_eq!(
            fs::read_to_string(&archive_path).unwrap(),
            format!("# {today}\n\n## inbox\n\n### a\n")
        );

        app.update(Action::Undo);
        assert_eq!(listing(&app), ["[inbox]", "a.md", "[work]", "b.md"]);
        assert!(!archive_path.exists());
    }
}
