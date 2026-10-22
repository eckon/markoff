//! App state, the run loop, and what each [`Action`] does to it and to the disk. What needs
//! `$EDITOR` is in [`editor`], the archive page in [`archive`].

use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use color_eyre::{Result, eyre::eyre};
use jiff::civil::Date;
use ratatui::{
    DefaultTerminal,
    crossterm::event::{self, Event, KeyCode, KeyEventKind},
};

pub mod archive;
mod editor;

use archive::ArchivePage;
use editor::EditorRequest;

use crate::{
    cursor, edit, heatmap,
    keymap::{self, Action, Keymap},
    load,
    op::{self, Op},
    state::{self, StateFile},
    task::{Section, Task},
    ui,
    watch::RootWatcher,
};

/// How long to wait for a key before checking the file watcher again.
const POLL_INTERVAL: Duration = Duration::from_millis(50);

/// One line in the list. The cursor moves over these.
///
/// Section headers are rows too, so the cursor can land on an empty section (to add a task or
/// move a section).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Row {
    Section(usize),
    /// Section index, task index within that section.
    Task(usize, usize),
}

/// Fills the screen, switched with `Tab` / `S-Tab` in this order. The help is an overlay.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    Tasks,
    /// The done tasks: heatmap and list, see [`crate::ui`].
    Archive,
}

impl Page {
    pub const ALL: [Page; 2] = [Page::Tasks, Page::Archive];

    pub fn name(self) -> &'static str {
        match self {
            Page::Tasks => "tasks",
            Page::Archive => "archive",
        }
    }
}

/// Message for the status, cleared on the next key press.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    Error(String),
    Info(String),
}

/// One user action on the undo stack.
#[derive(Debug)]
struct UndoStep {
    ops: Vec<Op>,
    /// Task file or section directory the cursor was on before the action. Undo puts it back
    /// there: the undo restores the old names, so the path exists again.
    cursor: Option<PathBuf>,
}

/// Application state.
#[derive(Debug)]
pub struct App {
    running: bool,
    /// Directory with the sections, `.markoff` and `.archive/`.
    root: PathBuf,
    pub sections: Vec<Section>,
    /// `.markoff` as read with `sections`, the base for the order an action writes.
    state_file: StateFile,
    /// Index into [`App::rows`].
    pub cursor: usize,
    pub keymap: Keymap,
    /// Applied actions, newest last. Undo applies the inverse of the last one.
    undo: Vec<UndoStep>,
    pub status: Option<Status>,
    /// Help overlay is open.
    pub show_help: bool,
    /// `p` hid the preview next to the list.
    pub hide_preview: bool,
    /// `.` shows the tasks that are not due, see [`Task::due`].
    pub show_hidden: bool,
    /// The local date, read from the clock once per loop so a frame never mixes two days, and
    /// tests can set it. A placeholder until the first loop.
    pub today: Date,
    pub page: Page,
    pub archive_page: ArchivePage,
    editor_request: Option<EditorRequest>,
    /// `/` filter, empty for none. Applied in [`App::rows`], so everything that works on rows
    /// sees only what matches.
    pub filter: String,
    /// The filter input at the bottom is open and takes the keys.
    pub filter_input: bool,
    /// First list line on screen. Kept between frames so the view only scrolls when the
    /// cursor would leave it, see [`ui::render`].
    pub list_offset: usize,
    /// The done tasks, from the archive.
    pub archive: crate::archive::Archive,
    /// Tasks done per day, from the archive.
    pub days: heatmap::Days,
}

impl App {
    /// Without a state file, [`App::load`] reads the real one.
    pub fn new(root: PathBuf, sections: Vec<Section>) -> Self {
        let state_file = StateFile {
            path: root.join(state::FILE),
            content: None,
        };
        Self {
            running: true,
            root,
            sections,
            state_file,
            cursor: 0,
            keymap: Keymap::default(),
            undo: Vec::new(),
            status: None,
            show_help: false,
            hide_preview: false,
            show_hidden: false,
            today: Date::ZERO,
            page: Page::Tasks,
            archive_page: ArchivePage::default(),
            editor_request: None,
            filter: String::new(),
            filter_input: false,
            list_offset: 0,
            archive: crate::archive::Archive::new(),
            days: heatmap::Days::new(),
        }
    }

    /// Loads `root` and normalizes it, renames are mentioned in the status.
    pub fn load(root: PathBuf) -> Result<Self> {
        let mut app = Self::new(root, Vec::new());
        app.reload(None)?;
        app.normalize_before_action();
        Ok(app)
    }

    /// Main loop: draw if needed, wait briefly for a key, check for outside changes.
    ///
    /// Keys are read on this thread (no background reader), so the TUI can hand the
    /// terminal to `$EDITOR` without the two fighting over stdin.
    pub fn run(mut self, terminal: &mut DefaultTerminal) -> Result<()> {
        let mut watcher = RootWatcher::new(&self.root)?;
        // Only redraw after something happened, not on every poll.
        let mut dirty = true;

        while self.running {
            // At midnight the app header, the due tasks and the archive's today move on.
            let today = jiff::Zoned::now().date();
            if today != self.today {
                self.today = today;
                dirty = true;
            }
            if dirty {
                let mut drawn = None;
                terminal.draw(|frame| drawn = Some(ui::render(&self, frame)))?;
                if let Some(drawn) = drawn {
                    self.list_offset = drawn.list_offset;
                    self.archive_page.columns = drawn.heatmap_columns;
                    self.archive_page.offset = drawn.archive_offset;
                }
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
    /// appeared or disappeared. A task that is gone leaves it in its section, see
    /// [`Self::neighbor_in_section`].
    fn reload_keep_selection(&mut self) -> Result<()> {
        let selected = self.selected().map(|row| self.row_path(row));
        let neighbor = self.neighbor_in_section();
        self.reload(None)?;
        for path in [selected, neighbor].iter().flatten() {
            if self.select_path(path) {
                break;
            }
        }
        Ok(())
    }

    /// Where the cursor goes when its task goes away, so it never ends up in another section
    /// by itself: the next task, else the row before, a task or the section header. `None` on a
    /// section, its row number stays.
    fn neighbor_in_section(&self) -> Option<PathBuf> {
        let rows = self.rows();
        let Some(&Row::Task(s, _)) = rows.get(self.cursor) else {
            return None;
        };
        let neighbor = match rows.get(self.cursor + 1) {
            Some(&Row::Task(next, t)) if next == s => Row::Task(next, t),
            _ => rows[self.cursor - 1],
        };
        Some(self.row_path(neighbor))
    }

    /// Path behind a row: the task file or the section directory. Never touches the disk,
    /// only keeps the cursor on the right row, e.g. after undo.
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
            // change tasks that are dimmed behind it. `Esc` by its code, on the task list it has
            // no binding.
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

    /// Runs an action. Errors end up in the status instead of ending the app.
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
        if self.page == Page::Archive && self.update_archive(action) {
            return Ok(());
        }
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
            self.status = Some(Status::Info("clear the filter first (/ Esc)".to_string()));
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
            // Only on the archive page, see [`Self::update_archive`].
            Action::Left
            | Action::Right
            | Action::SkipLeft
            | Action::SkipDown
            | Action::SkipUp
            | Action::SkipRight
            | Action::FocusList
            | Action::FocusHeatmap => {}
            // A section hidden by the filter has no header to jump to.
            Action::Up
            | Action::Down
            | Action::Top
            | Action::Bottom
            | Action::PrevSection
            | Action::NextSection
            | Action::JumpSection(_) => {
                let lines: Vec<cursor::Line> = (self.rows().into_iter())
                    .map(|row| match row {
                        Row::Section(s) => cursor::Line::Section(s),
                        Row::Task(..) => cursor::Line::Task,
                    })
                    .collect();
                self.cursor = cursor::moved(&lines, self.cursor, action);
            }
            Action::MoveDown | Action::MoveUp => self.move_selected(action == Action::MoveDown)?,
            Action::Open => self.open(),
            Action::NewBelow | Action::NewAbove => self.new_task(action == Action::NewBelow),
            Action::NewSection => self.new_section(),
            Action::Done => self.done()?,
            Action::Delete => self.delete()?,
            Action::Undo => self.undo()?,
            Action::Help => self.show_help = true,
            Action::Preview => self.hide_preview = !self.hide_preview,
            Action::ShowHidden => self.toggle_hidden(),
            Action::PrevPage | Action::NextPage => {
                let count = Page::ALL.len();
                let step = if action == Action::NextPage {
                    1
                } else {
                    count - 1
                };
                let at = Page::ALL.iter().position(|&page| page == self.page);
                self.page = Page::ALL[(at.unwrap_or(0) + step) % count];
                if self.page == Page::Archive {
                    self.open_archive();
                }
            }
            Action::Quit => self.running = false,
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
        if self.page == Page::Archive {
            change(&mut self.archive_page.filter);
            self.reset_archive_list();
            return;
        }
        let selected = self.selected().map(|row| self.row_path(row));
        change(&mut self.filter);
        self.cursor = 0;
        if let Some(path) = selected {
            self.select_path(&path);
        }
    }

    /// Renames all tasks into shape and cleans the state file with [`edit::normalize`].
    /// Returns the renames.
    ///
    /// Not added to undo: undoing it would only be redone by the next action. The cursor
    /// follows a renamed task.
    fn normalize(&mut self) -> Result<Vec<(PathBuf, PathBuf)>> {
        let ops = edit::normalize(&self.sections, &self.state_file);
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
    /// Renames are mentioned in the status. A failure is reported there too, everything
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
            _ => return Ok(()),
        };
        let date = self.today;
        if self.sections[s].tasks[t].repeat.is_some() {
            let ops = edit::repeat_done(&self.sections, s, t, date, &self.state_file);
            if self.apply(ops)? {
                self.status = Some(Status::Info("done until next time (u to undo)".to_string()));
            }
            return Ok(());
        }
        let section = &self.sections[s].name;
        let dir = (self.root.join(crate::archive::DIR))
            .join(date.to_string())
            .join(section);
        let mut missing: Vec<PathBuf> = (dir.ancestors())
            .take_while(|dir| !dir.exists())
            .map(Path::to_path_buf)
            .collect();
        missing.reverse();
        let taken: Vec<String> = (self.archive.get(&date).into_iter().flatten())
            .filter(|done| &done.section == section)
            .map(|done| done.task.stem().to_string())
            .collect();

        let ops = edit::archive(
            &self.sections,
            s,
            t,
            &dir,
            &missing,
            &taken,
            &self.state_file,
        );
        self.apply(ops)?;
        self.status = Some(Status::Info("task archived (u to undo)".to_string()));
        Ok(())
    }

    /// `J` / `K`: moves the task or section under the cursor one place, the cursor follows.
    /// Nothing happens at the edges.
    fn move_selected(&mut self, down: bool) -> Result<()> {
        let file = &self.state_file;
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
            None => return Ok(()),
        };
        self.apply(ops)?;
        self.select_path(&moved_to);
        Ok(())
    }

    /// Where `J` / `K` put task `t` of section `s`, as section and index among the other
    /// tasks there. At a section edge it goes into the neighbouring section, first or last.
    /// `None` at the very top or bottom.
    ///
    /// Hidden tasks are stepped over, so the task always passes one that can be seen.
    fn task_target(&self, s: usize, t: usize, down: bool) -> Option<(usize, usize)> {
        let tasks = &self.sections[s].tasks;
        let shown = |&i: &usize| self.shown(&tasks[i]);
        if down {
            match (t + 1..tasks.len()).find(shown) {
                Some(next) => Some((s, next)),
                None => (s + 1 < self.sections.len()).then_some((s + 1, 0)),
            }
        } else {
            match (0..t).rev().find(shown) {
                Some(previous) => Some((s, previous)),
                None => {
                    let previous = s.checked_sub(1)?;
                    Some((previous, self.sections[previous].tasks.len()))
                }
            }
        }
    }

    /// `.`: says what changed, the list alone does not show it when nothing is hidden.
    fn toggle_hidden(&mut self) {
        self.show_hidden = !self.show_hidden;
        let date = self.today;
        let hidden = (self.sections.iter().flat_map(|section| &section.tasks))
            .filter(|task| !task.due(date))
            .count();
        let message = match (hidden, self.show_hidden) {
            (0, _) => "no hidden tasks".to_string(),
            (_, true) => format!("hidden tasks shown ({hidden})"),
            (_, false) => "hidden tasks hidden".to_string(),
        };
        self.status = Some(Status::Info(message));
    }

    /// Whether `task` is in the list: when it is due, or `.` shows the hidden ones.
    fn shown(&self, task: &Task) -> bool {
        self.show_hidden || task.due(self.today)
    }

    /// `D`: deletes the task without archiving. Undo brings it back.
    fn delete(&mut self) -> Result<()> {
        let (s, t) = match self.selected() {
            Some(Row::Task(s, t)) => (s, t),
            Some(Row::Section(s)) => return self.delete_section(s),
            None => return Ok(()),
        };
        // The cursor goes to the next task (like vim), see [`Self::apply`].
        let ops = edit::delete(&self.sections, s, t, &self.state_file);
        self.apply(ops)?;
        self.status = Some(Status::Info("task deleted (u to undo)".to_string()));
        Ok(())
    }

    /// Only empty sections, so `D` never takes tasks with it.
    fn delete_section(&mut self, s: usize) -> Result<()> {
        let section = &self.sections[s];
        let name = section.name.clone();
        if !section.tasks.is_empty() {
            return Err(eyre!("{name} is not empty"));
        }
        // Cursor stays on the same row, which is now the next section.
        let ops = edit::delete_section(&self.sections, s, &self.state_file);
        self.apply(ops)?;
        self.status = Some(Status::Info("section deleted (u to undo)".to_string()));
        Ok(())
    }

    fn undo(&mut self) -> Result<()> {
        let Some(step) = self.undo.last() else {
            return Err(eyre!("nothing to undo"));
        };
        // Only pop once the inverse worked, so a failed undo can be retried.
        op::apply_all(&op::inverse_all(&step.ops))?;
        let cursor = step.cursor.clone();
        self.undo.pop();
        self.reload(cursor.as_deref())
    }

    /// Applies ops, records them for undo together with the cursor position, and reloads,
    /// keeping the selection, see [`Self::reload_keep_selection`].
    /// Without ops nothing changed, so there is no undo step either. Returns whether anything
    /// changed, so the caller only reports what really happened.
    fn apply(&mut self, ops: Vec<Op>) -> Result<bool> {
        if ops.is_empty() {
            return Ok(false);
        }
        op::apply_all(&ops)?;
        // `sections` still shows the disk before the ops until the reload, so the cursor and
        // its neighbor are found in it.
        let cursor = self.selected().map(|row| self.row_path(row));
        self.undo.push(UndoStep { ops, cursor });
        self.reload_keep_selection()?;
        Ok(true)
    }

    /// Reads the root again and puts the cursor on `path` if it still exists, else keeps the
    /// row number, clamped. The disk is the source of truth, `sections` is only a view of it.
    fn reload(&mut self, path: Option<&Path>) -> Result<()> {
        (self.sections, self.state_file) = load::load(&self.root)?;
        self.archive = load::archive(&self.root)?;
        self.days = heatmap::days(&self.archive);
        self.cursor = self.cursor.min(self.rows().len().saturating_sub(1));
        let archive_rows = self.archive_rows(self.archive_page.columns).len();
        self.archive_page.cursor = self.archive_page.cursor.min(archive_rows.saturating_sub(1));
        if let Some(path) = path {
            self.select_path(path);
        }
        Ok(())
    }

    /// Moves the cursor to the task file or section directory at `path`, if it exists.
    /// Returns whether it did.
    fn select_path(&mut self, path: &Path) -> bool {
        let found = self
            .rows()
            .into_iter()
            .position(|row| self.row_path(row) == path);
        if let Some(index) = found {
            self.cursor = index;
        }
        found.is_some()
    }

    /// All rows in display order: each section header with its tasks. A filter keeps matching
    /// tasks under their header and hides the rest.
    /// Computed on demand, so it never gets out of sync with `sections`.
    pub fn rows(&self) -> Vec<Row> {
        let mut rows = Vec::new();
        for (s, section) in self.sections.iter().enumerate() {
            let tasks: Vec<Row> = (0..section.tasks.len())
                .filter(|&t| {
                    let task = &section.tasks[t];
                    self.shown(task) && task.matches(&self.filter)
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

    /// The filter of the current page, each page has its own.
    pub fn page_filter(&self) -> &str {
        match self.page {
            Page::Tasks => &self.filter,
            Page::Archive => &self.archive_page.filter,
        }
    }

    /// Tasks archived today, shown in the app header.
    pub fn done_today(&self) -> usize {
        self.days.get(&self.today).copied().unwrap_or(0)
    }
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
        state::{self, State},
        task::Task,
    };

    /// The date tests run on, so they never depend on the clock.
    pub const TODAY: Date = jiff::civil::date(2026, 10, 22);

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
    /// `# a`. The state file lists everything in the given order.
    pub(super) fn app_with_files(files: &[&str]) -> (TempDir, App) {
        let dir = TempDir::new().unwrap();
        let root = dir.path();
        let mut state = State::default();
        for file in files {
            let (section, name) = file.split_once('/').expect("tasks live in a section");
            if state.section_position(section).is_none() {
                state.sections.push(state::Section {
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
            let order = state.sections.iter_mut().find(|s| s.name == section);
            order.unwrap().tasks.push(state::Task {
                name: stem.to_string(),
                last_done: None,
            });
        }
        fs::write(root.join(state::FILE), state.format()).unwrap();
        let mut app = App::load(root.to_path_buf()).unwrap();
        app.today = TODAY;
        (dir, app)
    }

    pub(super) fn state_file(dir: &TempDir) -> String {
        fs::read_to_string(dir.path().join(state::FILE)).unwrap()
    }

    /// File names on disk in row order, sections as `[name]`.
    pub(super) fn listing(app: &App) -> Vec<String> {
        app.rows()
            .into_iter()
            .map(|row| match row {
                Row::Section(s) => format!("[{}]", app.sections[s].name),
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
    fn pages_wrap_around_and_keep_task_keys_to_the_list() {
        let (_dir, mut app) = app_with_files(&["work/a.md"]);
        app.cursor = 1;
        app.handle_event(Event::Key(KeyCode::Tab.into()));
        assert_eq!(app.page, Page::Archive);

        app.handle_event(Event::Key(KeyCode::Char('X').into()));
        assert_eq!(listing(&app), ["[work]", "a.md"], "X did not archive");

        app.handle_event(Event::Key(KeyCode::Tab.into()));
        assert_eq!(app.page, Page::Tasks, "wraps around");
        app.handle_event(Event::Key(KeyCode::BackTab.into()));
        assert_eq!(app.page, Page::Archive);
    }

    #[test]
    fn preview_toggles_on_each_page_of_its_own() {
        let (_dir, mut app) = app_with_files(&["work/a.md"]);
        app.update(Action::Preview);
        assert!(app.hide_preview);
        app.update(Action::Preview);
        assert!(!app.hide_preview);
        app.update(Action::Preview);

        // On the archive page only in the list, and not the task list's.
        app.update(Action::NextPage);
        app.update(Action::Preview);
        assert!(!app.archive_page.hide_preview, "not in the heatmap");
        app.update(Action::FocusList);
        app.update(Action::Preview);
        assert!(app.archive_page.hide_preview);
        assert!(app.hide_preview, "the task list's stays");
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
        assert_eq!(state_file(&dir), "# work\na\n");
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
        fs::write(dir.path().join(state::FILE), "# work\ny\nz\nb\n").unwrap();
        app.reload_keep_selection().unwrap();

        assert_eq!(listing(&app), ["[work]", "y.md", "z.md", "b.md"]);
        assert_eq!(app.cursor, 3, "still on b.md");
    }

    #[test]
    fn done_dates_go_where_the_task_goes() {
        let (dir, mut app) = app_with_files(&["work/a.md", "work/b.md", "home/c.md"]);
        let state = "# work\na 2026-10-01\nb\n\n# home\nc 2026-10-02\n";
        fs::write(dir.path().join(state::FILE), state).unwrap();
        app.reload(None).unwrap();

        // Into the next section: the line moves along.
        app.cursor = 1;
        app.update(Action::MoveDown);
        app.update(Action::MoveDown);
        assert_eq!(
            state_file(&dir),
            "# work\nb\n\n# home\na 2026-10-01\nc 2026-10-02\n"
        );

        // A new title renames the file, the date follows on the next write.
        fs::write(dir.path().join("home/c.md"), "# d").unwrap();
        app.reload_keep_selection().unwrap();
        app.update(Action::MoveDown);
        assert_eq!(
            state_file(&dir),
            "# work\nb\n\n# home\nd 2026-10-02\na 2026-10-01\n"
        );

        // Deleted, its date goes with it.
        app.update(Action::Delete);
        assert_eq!(state_file(&dir), "# work\nb\n\n# home\nd 2026-10-02\n");
    }

    #[test]
    fn cursor_stays_in_its_section_when_its_task_goes() {
        let (dir, mut app) = app_with_files(&["work/a.md", "work/b.md", "home/c.md"]);
        // The last task of `work`: the one before, not `home`.
        app.cursor = 2;
        app.update(Action::Delete);
        assert_eq!(app.selected(), Some(Row::Task(0, 0)), "{:?}", listing(&app));
        // Its only task: onto the section header.
        app.update(Action::Done);
        assert_eq!(app.selected(), Some(Row::Section(0)), "{:?}", listing(&app));
        // Gone from outside too.
        app.cursor = 2;
        fs::remove_file(dir.path().join("home/c.md")).unwrap();
        app.reload_keep_selection().unwrap();
        assert_eq!(app.selected(), Some(Row::Section(1)));
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
        assert_eq!(state_file(&dir), "# work\nb\n");
        assert_eq!(app.cursor, 1, "cursor moves onto the next task");
        assert_eq!(
            app.status,
            Some(Status::Info("task deleted (u to undo)".to_string()))
        );

        app.update(Action::Undo);
        assert_eq!(listing(&app), ["[work]", "a.md", "b.md"]);
        assert_eq!(state_file(&dir), "# work\na\nb\n");
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
        assert_eq!(state_file(&dir), "# work\na\n\n# x\nb\n");
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

        // Back to normal keys, `Esc` does nothing, `/` and `Esc` clear.
        app.handle_event(key(KeyCode::Char('j')));
        app.handle_event(key(KeyCode::Esc));
        assert_eq!(app.filter, "b");
        app.handle_event(key(KeyCode::Char('/')));
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
                Some(Status::Info("clear the filter first (/ Esc)".to_string())),
                "{action:?}"
            );
        }
        assert_eq!(app.editor_request, None);
        app.update(Action::FilterClear);
        assert_eq!(listing(&app), ["[work]", "a.md", "b.md"]);
    }

    #[test]
    fn startup_normalizes_and_mentions_renames() {
        // Old numbered names, no state file yet.
        let dir = TempDir::new().unwrap();
        let work = dir.path().join("work");
        fs::create_dir_all(dir.path().join("01-home")).unwrap();
        fs::create_dir_all(&work).unwrap();
        fs::write(work.join("02-b.md"), "# b").unwrap();
        fs::write(work.join("01-a.md"), "# a").unwrap();
        fs::write(dir.path().join("01-home/old.md"), "# Old task").unwrap();
        let app = App::load(dir.path().to_path_buf()).unwrap();

        assert_eq!(
            listing(&app),
            ["[01-home]", "old-task.md", "[work]", "a.md", "b.md"]
        );
        assert_eq!(state_file(&dir), "# 01-home\nold-task\n\n# work\na\nb\n");
        assert_eq!(
            app.status,
            Some(Status::Info(
                "3 files renamed to match their titles".to_string()
            ))
        );
    }

    #[test]
    fn startup_cleans_the_state_file() {
        let (dir, _) = app_with_files(&["work/a.md", "work/b.md", "home/", "x/"]);
        fs::write(
            dir.path().join(state::FILE),
            "# x\n# work\ngone\nb\n# gone\n",
        )
        .unwrap();

        // Not listed: to the end, by name. Listed but gone: dropped. No rename, no message.
        let app = App::load(dir.path().to_path_buf()).unwrap();
        assert_eq!(listing(&app), ["[x]", "[work]", "b.md", "a.md", "[home]"]);
        assert_eq!(state_file(&dir), "# x\n\n# work\nb\na\n\n# home\n");
        assert_eq!(app.status, None);
    }

    #[test]
    fn reload_from_watcher_does_not_write() {
        let (dir, mut app) = app_with_files(&["work/a.md"]);
        fs::write(dir.path().join("work/draft.md"), "# New").unwrap();
        app.reload_keep_selection().unwrap();
        assert_eq!(listing(&app), ["[work]", "a.md", "draft.md"]);
        assert_eq!(state_file(&dir), "# work\na\n");
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
        assert_eq!(state_file(&dir), "# work\nb\na\n\n# home\nc\n");
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
        assert_eq!(state_file(&dir), "# home\nb\n\n# work\na\n\n# x\n");

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
        fs::write(dir.path().join("work/a.md"), "# a\n\nnotes\n").unwrap();
        let archive = dir.path().join(".archive");
        let day = archive.join(TODAY.to_string());

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
        assert_eq!(state_file(&dir), "# work\n\n# home\n");
        assert_eq!(
            fs::read_to_string(day.join("work/a.md")).unwrap(),
            "# a\n\nnotes\n",
            "moved as it was"
        );
        assert!(day.join("home/b.md").exists());

        app.update(Action::Undo);
        assert_eq!(listing(&app), ["[work]", "[home]", "b.md"]);
        assert_eq!(app.done_today(), 1);
        assert!(
            !day.join("home").exists(),
            "the directory it needed is gone"
        );

        app.update(Action::Undo);
        assert_eq!(listing(&app), ["[work]", "a.md", "[home]", "b.md"]);
        assert!(!archive.exists());
    }

    #[test]
    fn done_twice_a_day_gets_a_number() {
        let (dir, mut app) = app_with_files(&["work/a.md"]);
        let day = dir.path().join(".archive").join(TODAY.to_string());
        app.cursor = 1;
        app.update(Action::Done);
        // The same task again, written outside markoff.
        fs::write(dir.path().join("work/a.md"), "").unwrap();
        app.reload(None).unwrap();

        app.cursor = 1;
        app.update(Action::Done);
        assert!(day.join("work/a.md").exists());
        assert!(day.join("work/a-2.md").exists());
        assert_eq!(app.done_today(), 2);
    }

    #[test]
    fn hidden_tasks_only_with_dot() {
        let (dir, mut app) = app_with_files(&["work/a.md", "work/water.md", "work/b.md"]);
        fs::write(dir.path().join("work/water.md"), "# Water `* * *`\n").unwrap();
        let state = format!("# work\na\nwater {TODAY}\nb\n");
        fs::write(dir.path().join(state::FILE), &state).unwrap();
        app.reload(None).unwrap();
        assert_eq!(listing(&app), ["[work]", "a.md", "b.md"]);

        // `.` says what it did, the list alone would not when nothing is hidden.
        let info = |text: &str| Some(Status::Info(text.to_string()));
        app.update(Action::ShowHidden);
        assert_eq!(app.status, info("hidden tasks shown (1)"));
        app.update(Action::ShowHidden);
        assert_eq!(app.status, info("hidden tasks hidden"));
        let (_empty_dir, mut empty) = app_with_files(&["work/a.md"]);
        empty.update(Action::ShowHidden);
        assert_eq!(empty.status, info("no hidden tasks"));

        // `J` / `K` step over it, so the task visibly moves.
        app.cursor = 1;
        app.update(Action::MoveDown);
        assert_eq!(state_file(&dir), format!("# work\nwater {TODAY}\nb\na\n"));
        app.update(Action::MoveUp);
        assert_eq!(state_file(&dir), format!("# work\nwater {TODAY}\na\nb\n"));
        app.update(Action::Undo);
        app.update(Action::Undo);

        // The filter does not find it, unless `.` shows it.
        type_filter(&mut app, "water");
        assert_eq!(listing(&app), Vec::<String>::new());
        app.update(Action::ShowHidden);
        assert_eq!(listing(&app), ["[work]", "water.md"]);
        app.update(Action::FilterClear);
        assert_eq!(listing(&app), ["[work]", "a.md", "water.md", "b.md"]);

        // While shown, `J` / `K` take it like any task.
        app.cursor = 1;
        app.update(Action::MoveDown);
        assert_eq!(state_file(&dir), format!("# work\nwater {TODAY}\na\nb\n"));
        app.update(Action::ShowHidden);
        assert_eq!(listing(&app), ["[work]", "a.md", "b.md"]);
    }

    #[test]
    fn repeating_task_done_until_next_time() {
        let (dir, mut app) = app_with_files(&["work/a.md", "work/water.md"]);
        let water = "# Water `* * *`\n";
        fs::write(dir.path().join("work/water.md"), water).unwrap();
        app.reload(None).unwrap();

        // Never done: due.
        app.cursor = 2;
        app.update(Action::Done);
        assert_eq!(
            app.status,
            Some(Status::Info("done until next time (u to undo)".to_string()))
        );
        // Hidden, the date in the state file, the task and the archive untouched.
        assert_eq!(listing(&app), ["[work]", "a.md"]);
        assert_eq!(state_file(&dir), format!("# work\na\nwater {TODAY}\n"));
        let content = fs::read_to_string(dir.path().join("work/water.md")).unwrap();
        assert_eq!(content, water);
        assert!(!dir.path().join(".archive").exists());
        assert_eq!(app.done_today(), 0, "not in the archive");

        // Again while `.` shows it: nothing changes, no undo step to take back.
        app.update(Action::ShowHidden);
        app.cursor = 2;
        let steps = app.undo.len();
        app.update(Action::Done);
        assert_eq!(app.undo.len(), steps);
        assert_eq!(
            app.status,
            Some(Status::Info("hidden tasks shown (1)".to_string())),
            "no claim"
        );
        assert_eq!(state_file(&dir), format!("# work\na\nwater {TODAY}\n"));
        app.update(Action::ShowHidden);

        app.update(Action::Undo);
        assert_eq!(listing(&app), ["[work]", "a.md", "water.md"]);
        assert_eq!(app.selected(), Some(Row::Task(0, 1)), "back on it");
        assert_eq!(state_file(&dir), "# work\na\nwater\n");

        // Done yesterday: due again today.
        fs::write(
            dir.path().join(state::FILE),
            format!("# work\na\nwater {}\n", TODAY.yesterday().unwrap()),
        )
        .unwrap();
        app.reload(None).unwrap();
        assert_eq!(listing(&app), ["[work]", "a.md", "water.md"]);
    }
}
