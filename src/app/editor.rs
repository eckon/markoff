//! What needs `$EDITOR`: opening a task, writing a new task or section, renaming a section.
//! An action only asks for it, the run loop owns the terminal and opens the editor, then what
//! was written is turned into ops here.

use std::path::PathBuf;

use color_eyre::{Result, eyre::eyre};
use ratatui::DefaultTerminal;

use super::{App, Row, Status};
use crate::{
    edit, editor,
    task::{HEADING, split_heading},
};

/// Work that needs the editor. Set by `update`, handled by the run loop, which owns
/// the terminal. Keeps `update` testable without a terminal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum EditorRequest {
    /// `<CR>`: edit an existing task.
    Open(PathBuf),
    /// `o` / `O`: write a new task, inserted at `index` in section `section`.
    Create { section: usize, index: usize },
    /// `N`: write a new section, inserted at `index` among the sections.
    CreateSection(usize),
    /// `<CR>` on a section header: rename section `section`.
    RenameSection(usize),
}

impl App {
    /// Opens the editor for `request` and applies what was written.
    pub(super) fn run_editor(
        &mut self,
        terminal: &mut DefaultTerminal,
        request: EditorRequest,
    ) -> Result<()> {
        match request {
            EditorRequest::Open(path) => {
                editor::open(terminal, &path, &[]).and_then(|()| self.after_edit())
            }
            EditorRequest::Create { section, index } => {
                let draft = format!("{HEADING}\n");
                editor::create(terminal, &self.sections[section].dir, &draft)
                    .and_then(|content| self.after_create(section, index, &content))
            }
            EditorRequest::CreateSection(index) => {
                let draft = edit::section_draft("");
                editor::create(terminal, &self.root, &draft)
                    .and_then(|content| self.after_create_section(index, &content))
            }
            EditorRequest::RenameSection(section) => {
                let draft = edit::section_draft(&self.sections[section].name);
                editor::rename(terminal, &self.root, &draft)
                    .and_then(|content| self.after_rename_section(section, &content))
            }
        }
    }

    /// `<CR>`: edit the task, or rename the section, under the cursor.
    pub(super) fn open(&mut self) {
        match self.selected() {
            Some(Row::Task(s, t)) => {
                let path = self.sections[s].tasks[t].path.clone();
                self.editor_request = Some(EditorRequest::Open(path));
            }
            Some(Row::Section(s)) => self.editor_request = Some(EditorRequest::RenameSection(s)),
            None => {}
        }
    }

    /// `o` / `O`: a new task next to the selected task. From a section header both put it first
    /// in the section, so an empty section can get its first task.
    pub(super) fn new_task(&mut self, below: bool) {
        let (section, index) = match self.selected() {
            Some(Row::Task(section, task)) => (section, task + usize::from(below)),
            Some(Row::Section(section)) => (section, 0),
            None => return,
        };
        self.editor_request = Some(EditorRequest::Create { section, index });
    }

    /// `N`: a new section after the selected section. On top if there are no rows at all, so the
    /// first section can always be created.
    pub(super) fn new_section(&mut self) {
        let index = match self.selected() {
            Some(Row::Section(section) | Row::Task(section, _)) => section + 1,
            None => 0,
        };
        self.editor_request = Some(EditorRequest::CreateSection(index));
    }

    /// Reloads after the editor closed. Normalizing renames the file if its heading changed.
    fn after_edit(&mut self) -> Result<()> {
        self.reload(None)?;
        self.normalize_before_action();
        Ok(())
    }

    /// Inserts what was written in the editor for `o` / `O`.
    fn after_create(&mut self, section: usize, index: usize, content: &str) -> Result<()> {
        let Some(title) = split_heading(content).0 else {
            return Err(eyre!("aborted, empty title"));
        };
        // Normalizing keeps the order of tasks and sections, so `section` and `index` stay valid.
        self.normalize_before_action();
        // `sections` is from before the editor. If the files changed meanwhile, the ops refuse
        // and the text is lost, so say that clearly.
        let (ops, created) = edit::insert(
            &self.sections,
            section,
            index,
            &title,
            content,
            &self.state_file,
        );
        self.apply(ops)
            .map_err(|err| eyre!("task not added: {err}"))?;
        self.select_path(&created);
        Ok(())
    }

    /// Creates a section named by the heading written in the editor, at `index`.
    fn after_create_section(&mut self, index: usize, content: &str) -> Result<()> {
        let (slug, more) =
            edit::section_name(content).ok_or_else(|| eyre!("aborted, empty name"))?;
        self.ensure_no_section(&slug)?;
        // Normalizing keeps the order of sections, so `index` stays valid.
        self.normalize_before_action();
        let (ops, created) =
            edit::insert_section(&self.sections, &self.root, index, &slug, &self.state_file);
        self.apply(ops)?;
        self.select_path(&created);
        if more {
            self.status = Some(Status::Info(
                "section created, only the heading is used".to_string(),
            ));
        }
        Ok(())
    }

    /// Two sections with the same name would look the same in the list and the archive.
    fn ensure_no_section(&self, slug: &str) -> Result<()> {
        if self.sections.iter().any(|section| section.name == slug) {
            return Err(eyre!("a section named {slug} already exists"));
        }
        Ok(())
    }

    /// Renames section `s` to the heading written in the editor.
    fn after_rename_section(&mut self, s: usize, content: &str) -> Result<()> {
        let (slug, more) =
            edit::section_name(content).ok_or_else(|| eyre!("aborted, empty name"))?;
        if slug == self.sections[s].name {
            return Ok(());
        }
        self.ensure_no_section(&slug)?;
        // Normalizing keeps the order of sections, so `s` stays valid.
        self.normalize_before_action();
        let (ops, to) = edit::rename_section(&self.sections, s, &slug, &self.state_file);
        self.apply(ops)?;
        self.select_path(&to);
        if more {
            self.status = Some(Status::Info(
                "section renamed, only the heading is used".to_string(),
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::{
        app::tests::{app_with_files, listing, state_file},
        keymap::Action,
        state,
    };

    #[test]
    fn open_requests_editor_for_task_and_section() {
        let (dir, mut app) = app_with_files(&["work/a.md", "home/"]);
        app.update(Action::Open);
        assert_eq!(app.editor_request, Some(EditorRequest::RenameSection(0)));

        app.cursor = 2;
        app.update(Action::Open);
        assert_eq!(app.editor_request, Some(EditorRequest::RenameSection(1)));

        app.cursor = 1;
        app.update(Action::Open);
        assert_eq!(
            app.editor_request,
            Some(EditorRequest::Open(dir.path().join("work/a.md")))
        );
    }

    #[test]
    fn new_requests_create_next_to_the_cursor() {
        let (_dir, mut app) = app_with_files(&["work/a.md", "work/b.md", "home/"]);
        let create = |section, index| Some(EditorRequest::Create { section, index });
        // Rows: work, a, b, home
        for (cursor, action, expected) in [
            (1, Action::NewBelow, create(0, 1)),
            (1, Action::NewAbove, create(0, 0)),
            (2, Action::NewBelow, create(0, 2)),
            (2, Action::NewAbove, create(0, 1)),
            (0, Action::NewBelow, create(0, 0)),
            (0, Action::NewAbove, create(0, 0)),
            (3, Action::NewBelow, create(1, 0)),
            (3, Action::NewAbove, create(1, 0)),
        ] {
            app.cursor = cursor;
            app.update(action);
            assert_eq!(app.editor_request, expected, "cursor {cursor}, {action:?}");
        }
    }

    #[test]
    fn new_does_nothing_without_a_section() {
        let (_dir, mut app) = app_with_files(&[]);
        for action in [Action::NewBelow, Action::NewAbove] {
            app.update(action);
            assert_eq!(app.editor_request, None, "{action:?}");
            assert_eq!(app.status, None, "{action:?}");
        }
    }

    #[test]
    fn after_create_inserts_and_undo_removes() {
        let (dir, mut app) = app_with_files(&["work/a.md", "work/b.md"]);
        // `o` was pressed on `b`.
        app.cursor = 2;
        app.after_create(0, 2, "# New task\n\nnotes\n").unwrap();
        assert_eq!(listing(&app), ["[work]", "a.md", "b.md", "new-task.md"]);
        assert_eq!(state_file(&dir), "# work\na\nb\nnew-task\n");
        assert_eq!(app.cursor, 3, "cursor is on the new task");

        app.update(Action::Undo);
        assert_eq!(listing(&app), ["[work]", "a.md", "b.md"]);
        assert_eq!(
            app.cursor, 2,
            "cursor is back on `b`, where `o` was pressed"
        );
    }

    #[test]
    fn insert_at_the_top_touches_no_other_file() {
        let (dir, mut app) = app_with_files(&["work/a.md", "work/b.md", "work/c.md"]);
        app.cursor = 1;
        app.after_create(0, 0, "# New").unwrap();
        assert_eq!(listing(&app), ["[work]", "new.md", "a.md", "b.md", "c.md"]);
        assert_eq!(state_file(&dir), "# work\nnew\na\nb\nc\n");
        assert_eq!(app.cursor, 1, "cursor is on the new task");

        // One undo reverts the file and the order.
        app.update(Action::Undo);
        assert_eq!(listing(&app), ["[work]", "a.md", "b.md", "c.md"]);
        assert_eq!(state_file(&dir), "# work\na\nb\nc\n");
    }

    #[test]
    fn after_create_aborts_without_title() {
        let (_dir, mut app) = app_with_files(&["work/a.md"]);
        for content in ["# \n", "", "text but no heading\n"] {
            let err = app.after_create(0, 1, content).unwrap_err();
            assert_eq!(err.to_string(), "aborted, empty title");
        }
        assert_eq!(listing(&app), ["[work]", "a.md"]);
    }

    #[test]
    fn after_create_fails_on_changes_made_while_editing() {
        let (dir, mut app) = app_with_files(&["work/a.md", "work/b.md"]);
        app.cursor = 1;
        // A sync reorders while the editor is open. That order must not be overwritten.
        fs::write(dir.path().join(state::FILE), "# work\nb\na\n").unwrap();

        let err = app.after_create(0, 1, "# New").unwrap_err();
        assert!(err.to_string().starts_with("task not added: "), "{err}");
        assert!(!dir.path().join("work/new.md").exists(), "nothing written");
        assert_eq!(state_file(&dir), "# work\nb\na\n");
    }

    #[test]
    fn after_create_section_inserts_and_undo_removes() {
        let (dir, mut app) = app_with_files(&["work/a.md", "home/"]);
        let before = listing(&app);
        let draft = edit::section_draft;

        assert_eq!(
            app.after_create_section(1, &draft(""))
                .unwrap_err()
                .to_string(),
            "aborted, empty name"
        );
        assert_eq!(
            app.after_create_section(1, &draft("Home"))
                .unwrap_err()
                .to_string(),
            "a section named home already exists"
        );
        assert_eq!(listing(&app), before);

        // `N` was pressed on `a`: after `work`, before `home`.
        app.cursor = 1;
        let content = format!("{}\nnotes", draft("Garden"));
        app.after_create_section(1, &content).unwrap();
        assert_eq!(listing(&app), ["[work]", "a.md", "[garden]", "[home]"]);
        assert!(dir.path().join("garden").is_dir());
        assert_eq!(state_file(&dir), "# work\na\n\n# garden\n\n# home\n");
        assert_eq!(app.cursor, 2, "cursor on the new section");
        assert_eq!(
            app.status,
            Some(Status::Info(
                "section created, only the heading is used".to_string()
            ))
        );

        app.update(Action::Undo);
        assert_eq!(listing(&app), before);
        assert!(!dir.path().join("garden").exists());
        assert_eq!(app.cursor, 1, "back on `a`");
    }

    #[test]
    fn first_section_in_an_empty_root() {
        let (dir, mut app) = app_with_files(&[]);
        app.update(Action::NewSection);
        assert_eq!(app.editor_request, Some(EditorRequest::CreateSection(0)));

        app.after_create_section(0, &edit::section_draft("Work"))
            .unwrap();
        assert_eq!(listing(&app), ["[work]"]);
        assert_eq!(state_file(&dir), "# work\n");
        assert_eq!(app.cursor, 0);
    }

    #[test]
    fn new_section_requests_create_after_cursor_section() {
        let (_dir, mut app) = app_with_files(&["work/a.md", "home/b.md"]);
        // Rows: work, a, home, b.
        for (cursor, index) in [(0, 1), (1, 1), (2, 2), (3, 2)] {
            app.cursor = cursor;
            app.update(Action::NewSection);
            assert_eq!(
                app.editor_request,
                Some(EditorRequest::CreateSection(index)),
                "cursor {cursor}"
            );
        }
    }

    #[test]
    fn after_rename_section_renames_and_undo_reverts() {
        let (dir, mut app) = app_with_files(&["work/a.md", "home/"]);
        let before = listing(&app);
        let template = edit::section_draft("work");
        let draft = |name: &str| template.replace("# work", name);

        // Empty name, unchanged name and a clash change nothing.
        let (empty, same, clash) = (draft("# "), draft("# work"), draft("# Home"));
        assert_eq!(
            app.after_rename_section(0, &empty).unwrap_err().to_string(),
            "aborted, empty name"
        );
        app.after_rename_section(0, &same).unwrap();
        assert_eq!(
            app.after_rename_section(0, &clash).unwrap_err().to_string(),
            "a section named home already exists"
        );
        assert_eq!(listing(&app), before);
        assert!(app.undo.is_empty());

        // Text below the heading is dropped, the status says so.
        app.cursor = 1;
        let renamed = format!("{}\nnotes", draft("# Work Stuff"));
        app.after_rename_section(0, &renamed).unwrap();
        assert!(dir.path().join("work-stuff/a.md").exists());
        assert_eq!(state_file(&dir), "# work-stuff\na\n\n# home\n");
        assert_eq!(
            app.selected(),
            Some(Row::Section(0)),
            "cursor on the section"
        );
        assert_eq!(
            app.status,
            Some(Status::Info(
                "section renamed, only the heading is used".to_string()
            ))
        );

        app.update(Action::Undo);
        assert_eq!(listing(&app), before);
        assert_eq!(app.cursor, 1, "back on the task");
    }

    #[test]
    fn after_edit_renames_to_new_heading() {
        let (dir, mut app) = app_with_files(&["work/a.md", "work/b.md"]);
        app.cursor = 1;
        let path = app.sections[0].tasks[0].path.clone();

        // What the user does in the editor.
        fs::write(&path, "# Renamed task\n\nnotes").unwrap();
        app.after_edit().unwrap();

        assert_eq!(listing(&app), ["[work]", "renamed-task.md", "b.md"]);
        assert_eq!(
            state_file(&dir),
            "# work\nrenamed-task\nb\n",
            "keeps its place"
        );
        assert_eq!(app.cursor, 1, "cursor follows the renamed file");
        assert_eq!(app.sections[0].tasks[0].body, "notes");
    }
}
