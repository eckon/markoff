use std::{
    fs, io,
    path::{Path, PathBuf},
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
    edit,
    keymap::{Action, Keymap},
    load,
    op::{self, Op},
    task::{Section, Task},
    ui,
};

/// One line in the list. The cursor moves over these.
///
/// Section headers are rows too, so the cursor can land on an empty section (to paste into it).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Row {
    Section(usize),
    /// Section index, task index within that section.
    Task(usize, usize),
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
    /// Message for the status line, e.g. an error. Cleared on the next key press.
    pub status: Option<String>,
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
        }
    }

    pub fn load(root: PathBuf) -> Result<Self> {
        let sections = load::load(&root.join("todo"))?;
        Ok(Self::new(root, sections))
    }

    /// Main loop: draw, wait for input, update.
    ///
    /// Events are read on this thread (no background reader), so the TUI can hand the
    /// terminal to `$EDITOR` with `ratatui::restore()` and take it back with
    /// `ratatui::init()` without the two fighting over stdin.
    pub fn run(mut self, terminal: &mut DefaultTerminal) -> Result<()> {
        while self.running {
            terminal.draw(|frame| ui::render(&self, frame))?;
            self.handle_event(event::read()?);
        }
        Ok(())
    }

    fn handle_event(&mut self, event: Event) {
        // Only presses: some terminals also report key releases.
        if let Event::Key(key) = event
            && key.kind == KeyEventKind::Press
        {
            self.status = None;
            if let Some(action) = self.keymap.handle(key) {
                self.update(action);
            }
        }
    }

    /// Runs an action. Errors end up in the status line instead of ending the app.
    pub fn update(&mut self, action: Action) {
        if let Err(err) = self.try_update(action) {
            self.status = Some(err.to_string());
        }
    }

    fn try_update(&mut self, action: Action) -> Result<()> {
        match action {
            Action::Up => self.cursor = self.cursor.saturating_sub(1),
            Action::Down => {
                let last = self.rows().len().saturating_sub(1);
                self.cursor = (self.cursor + 1).min(last);
            }
            Action::Done => self.done()?,
            Action::Cut => self.cut()?,
            Action::PasteBelow => self.paste(false)?,
            Action::PasteAbove => self.paste(true)?,
            Action::Undo => self.undo()?,
            Action::Quit => self.running = false,
        }
        Ok(())
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
        let Some(task) = &self.register else {
            return Err(eyre!("nothing to paste"));
        };
        let (s, index) = match self.selected() {
            Some(Row::Task(s, t)) if above => (s, t),
            Some(Row::Task(s, t)) => (s, t + 1),
            // On a section header both `p` and `P` paste as its first task.
            Some(Row::Section(s)) => (s, 0),
            None => return Ok(()),
        };

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

    fn select_path(&mut self, path: &Path) {
        let found = self.rows().iter().position(|row| match *row {
            Row::Task(s, t) => self.sections[s].tasks[t].path == path,
            Row::Section(_) => false,
        });
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

fn created_path(ops: &[Op]) -> Option<PathBuf> {
    ops.iter().find_map(|op| match op {
        Op::Create { path, .. } => Some(path.clone()),
        _ => None,
    })
}

#[cfg(test)]
pub mod tests {
    use std::fs;

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
        assert_eq!(app.status.as_deref(), Some("nothing to undo"));
    }

    #[test]
    fn errors_go_to_status() {
        let (_dir, mut app) = app_with_files(&["010-a.md"]);
        app.update(Action::PasteBelow);
        assert_eq!(app.status.as_deref(), Some("nothing to paste"));
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
