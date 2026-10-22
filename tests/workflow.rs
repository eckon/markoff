//! End to end: the real binary in a pseudo terminal, driven by keys like a user would.
//!
//! Few long workflows instead of many small cases, the unit tests cover the details. Each one
//! snapshots the screen and the files on disk at a few checkpoints (`cargo insta review`).
//!
//! A step is what the user does, then what should appear:
//!
//! ```ignore
//! app.edit("# Buy milk\n"); // what gets typed in the editor the next key opens
//! app.press("o");
//! app.wait_for("• Buy milk");
//! ```

use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};

use portable_pty::{CommandBuilder, PtySize, native_pty_system};
use tempfile::TempDir;

const ROWS: u16 = 20;
const COLS: u16 = 80;

/// A running markoff on its own root, config and fake editor.
struct Markoff {
    root: TempDir,
    /// What the fake editor writes into the file it is opened on, set by [`Markoff::edit`].
    next_edit: PathBuf,
    screen: Arc<Mutex<vt100::Parser>>,
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    child: Box<dyn portable_pty::Child + Send + Sync>,
    // Both only kept alive: the pty's master side and the fake HOME.
    _master: Box<dyn portable_pty::MasterPty + Send>,
    _home: TempDir,
}

impl Markoff {
    fn start() -> Self {
        Self::start_with(&[])
    }

    /// Starts on a root that already has `files`, as `(path in root, content)`.
    fn start_with(files: &[(&str, &str)]) -> Self {
        let root = TempDir::new().unwrap();
        for (path, content) in files {
            write(&root.path().join(path), content);
        }
        // HOME and config are empty, so the user's config never leaks into the test.
        let home = TempDir::new().unwrap();
        let next_edit = home.path().join("next-edit.md");

        let pty = native_pty_system()
            .openpty(PtySize {
                rows: ROWS,
                cols: COLS,
                ..PtySize::default()
            })
            .unwrap();
        let mut command = CommandBuilder::new(env!("CARGO_BIN_EXE_markoff"));
        command.arg("--dir");
        command.arg(root.path());
        command.env_clear();
        command.env("PATH", std::env::var("PATH").unwrap());
        command.env("TERM", "xterm-256color");
        command.env("HOME", home.path());
        command.env("XDG_CONFIG_HOME", home.path());
        // Runs as `cp <next-edit> <file>`: "typing" in the editor replaces the whole file.
        command.env("VISUAL", format!("cp {}", next_edit.display()));
        let child = pty.slave.spawn_command(command).unwrap();

        let screen = Arc::new(Mutex::new(vt100::Parser::new(ROWS, COLS, 0)));
        let writer = Arc::new(Mutex::new(pty.master.take_writer().unwrap()));
        let mut reader = pty.master.try_clone_reader().unwrap();
        let (parser, answer) = (Arc::clone(&screen), Arc::clone(&writer));
        thread::spawn(move || {
            let mut buf = [0; 4096];
            while let Ok(n @ 1..) = reader.read(&mut buf) {
                let mut parser = parser.lock().unwrap();
                parser.process(&buf[..n]);
                // crossterm asks for the cursor position when coming back from the editor,
                // a real terminal answers that, vt100 only parses.
                if buf[..n].windows(4).any(|w| w == b"\x1b[6n") {
                    let (row, col) = parser.screen().cursor_position();
                    let reply = format!("\x1b[{};{}R", row + 1, col + 1);
                    answer.lock().unwrap().write_all(reply.as_bytes()).unwrap();
                }
            }
        });

        Markoff {
            root,
            next_edit,
            screen,
            writer,
            child,
            _master: pty.master,
            _home: home,
        }
    }

    /// Types `keys` in vim notation, like the README: `gg`, `<CR>`, `<S-Down>`, `<C-c>`.
    ///
    /// Wait for the screen after `<Esc>`: typed right after it, the next key would arrive as
    /// part of an escape sequence (`Alt+key`), like in any terminal.
    #[track_caller]
    fn press(&mut self, keys: &str) {
        let mut bytes = String::new();
        let mut rest = keys;
        while let Some(c) = rest.chars().next() {
            let special = rest.strip_prefix('<').and_then(|r| r.split_once('>'));
            if let Some((name, after)) = special {
                bytes += match name {
                    "CR" => "\r",
                    "Esc" => "\x1b",
                    "BS" => "\x7f",
                    "Up" => "\x1b[A",
                    "Down" => "\x1b[B",
                    "S-Up" => "\x1b[1;2A",
                    "S-Down" => "\x1b[1;2B",
                    "Home" => "\x1b[H",
                    "End" => "\x1b[F",
                    "Tab" => "\t",
                    "S-Tab" => "\x1b[Z",
                    "C-c" => "\x03",
                    _ => panic!("unknown key <{name}>"),
                };
                rest = after;
            } else {
                bytes.push(c);
                rest = &rest[c.len_utf8()..];
            }
        }
        let mut writer = self.writer.lock().unwrap();
        writer.write_all(bytes.as_bytes()).unwrap();
        writer.flush().unwrap();
    }

    /// Changes a file from outside, like another editor or a git pull would.
    fn write(&self, path: &str, content: &str) {
        write(&self.root.path().join(path), content);
    }

    /// Deletes a file from outside.
    fn remove(&self, path: &str) {
        fs::remove_file(self.root.path().join(path)).unwrap();
    }

    /// Sets what the next editor session saves.
    fn edit(&self, content: &str) {
        fs::write(&self.next_edit, content).unwrap();
    }

    /// Waits until the screen contains `text`, which may span lines. Waiting on what should
    /// appear instead of sleeping keeps the test fast and stable.
    ///
    /// The first line may be anywhere in a row, the next ones start the rows below it.
    /// Whatever is right of them is ignored, like the side panel.
    #[track_caller]
    fn wait_for(&self, text: &str) {
        let (first, next) = text.split_once('\n').unwrap_or((text, ""));
        let start = Instant::now();
        loop {
            let screen = self.screen();
            let rows: Vec<&str> = screen.lines().collect();
            let found = rows.iter().enumerate().any(|(i, row)| {
                row.contains(first)
                    && (next.is_empty()
                        || next.split('\n').enumerate().all(|(j, line)| {
                            rows.get(i + 1 + j).is_some_and(|row| row.starts_with(line))
                        }))
            });
            if found {
                return;
            }
            if start.elapsed() > Duration::from_secs(5) {
                panic!("timed out waiting for {text:?}, screen:\n{screen}");
            }
            thread::sleep(Duration::from_millis(10));
        }
    }

    fn screen(&self) -> String {
        let parser = self.screen.lock().unwrap();
        let rows: Vec<String> = parser
            .screen()
            .rows(0, COLS)
            .map(|row| row.trim_end().to_string())
            .collect();
        rows.join("\n")
    }

    /// The screen, then everything in the root: directories as `name/`, files with content.
    fn state(&self) -> String {
        let mut out = format!("{}\n", self.screen().trim_end());
        for path in entries(self.root.path()) {
            let name = path.strip_prefix(self.root.path()).unwrap().display();
            if path.is_dir() {
                out += &format!("\n--- {name}/\n");
            } else {
                out += &format!("\n--- {name}\n{}", fs::read_to_string(&path).unwrap());
            }
        }
        out
    }

    #[track_caller]
    fn quit(mut self) {
        self.press("q");
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(5) {
            if let Some(status) = self.child.try_wait().unwrap() {
                assert!(
                    status.success(),
                    "exit {status:?}, screen:\n{}",
                    self.screen()
                );
                return;
            }
            thread::sleep(Duration::from_millis(10));
        }
        self.child.kill().unwrap();
        panic!("did not quit, screen:\n{}", self.screen());
    }
}

fn write(path: &Path, content: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, content).unwrap();
}

/// Everything below `dir`, sorted, hidden entries included (`.markoff`, `.archive`).
fn entries(dir: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            found.extend(entries(&path));
        }
        found.push(path);
    }
    found.sort();
    found
}

/// Snapshots `state` with the date masked, it is in the header and the archive's file name.
macro_rules! snapshot {
    ($name:expr, $state:expr) => {
        insta::with_settings!({ filters => vec![
            (r"\d{4}-\d{2}-\d{2}", "[date]"),
            (r"\w{3} \d{1,2} \w{3} ─*", "[date] ─"),
        ]}, {
            insta::assert_snapshot!($name, $state);
        })
    };
}

/// The base functions from an empty root to quit: create, move, edit, rename, done, delete
/// and undo, on tasks and sections.
#[test]
fn lifecycle() {
    let mut app = Markoff::start();
    app.wait_for("0 open");
    snapshot!("lifecycle_1_empty", app.state());

    app.edit("# Home\n");
    app.press("N");
    app.wait_for("──[1]─ home");
    app.edit("# Buy milk\n");
    app.press("o");
    app.wait_for("• Buy milk");
    app.edit("# Call mom\n");
    app.press("o");
    app.wait_for("• Call mom");
    app.edit("# Pay rent\n");
    app.press("O");
    app.wait_for("• Pay rent\n     • Call mom");
    snapshot!("lifecycle_2_created", app.state());

    // Pay rent to the top, then edit Buy milk below it: the file follows the new title.
    app.press("KK");
    app.wait_for("• Pay rent\n     • Buy milk");
    app.press("j");
    app.edit("# Buy oat milk\n\nThe 1l one.\n");
    app.press("<CR>");
    app.wait_for("• Buy oat milk");

    // A second section after the first, with a task that then moves up into the first.
    app.press("G");
    app.wait_for("▌   • Call mom");
    app.edit("# Work\n");
    app.press("N");
    app.wait_for("──[2]─ work");
    app.edit("# Write report\n");
    app.press("o");
    app.wait_for("• Write report");
    app.press("K");
    app.wait_for("• Write report\n\n  ──[2]─ work");

    // Rename the first section from its header.
    app.press("gg");
    app.wait_for("▌──[1]─ home");
    app.edit("# House\n");
    app.press("<CR>");
    app.wait_for("──[1]─ house");
    snapshot!("lifecycle_3_organized", app.state());

    // The now empty second section is deleted, then a task is done and another deleted.
    app.press("}");
    app.wait_for("▌──[2]─ work");
    app.press("D");
    app.wait_for("section deleted");
    // `1` lands on the first task of the first section.
    app.press("1");
    app.wait_for("▌   • Pay rent");
    app.press("X");
    app.wait_for("✓ 1 done");
    app.press("D");
    app.wait_for("2 open");

    // A repeating task: `X` keeps it, only its date goes into `.markoff`, and hides it until
    // it is due again. `.` shows it.
    app.press("G");
    app.wait_for("▌   • Write report");
    app.edit("# Water plants `* * *`\n");
    app.press("o");
    app.wait_for("3 open");
    app.press("X");
    app.wait_for("done until next time");
    app.press(".");
    app.wait_for("⟳ Water plants");
    snapshot!("lifecycle_4_done_and_deleted", app.state());

    // Undo all five, newest first.
    app.press("u");
    app.wait_for("3 open");
    app.press("u");
    app.wait_for("2 open");
    app.press("u");
    app.wait_for("3 open");
    app.press("u");
    app.wait_for("✓ 0 done");
    app.press("u");
    app.wait_for("──[2]─ work");
    snapshot!("lifecycle_5_undone", app.state());

    app.quit();
}

/// Files written by hand and changed from outside while markoff runs.
#[test]
fn hand_made_data() {
    let mut app = Markoff::start_with(&[
        ("notes.md", "# Not a task, files in the root are ignored\n"),
        ("home/water-plants.md", "# Water plants\n"),
        ("work/review-pr.md", "# Review PR\n"),
        // Named by hand, renamed after its title on start.
        ("work/todo.md", "# Fix login bug\n\nOn mobile only.\n"),
        ("work/.scratch.md", "# Hidden, ignored\n"),
        // A stale entry, and todo.md is not listed at all.
        (
            ".markoff",
            "# home\nwater-plants\ngone\n\n# work\nreview-pr\n",
        ),
    ]);
    app.wait_for("• Fix login bug");
    snapshot!("hand_made_data_1_started", app.state());

    // Another editor adds, changes and deletes while markoff runs. It shows the changes but
    // renames nothing: that would break the other editor's next save.
    app.write("home/buy-bread.md", "# Buy bread\n");
    app.wait_for("• Buy bread");
    app.write("work/review-pr.md", "# Review PR 42\n");
    app.wait_for("• Review PR 42");
    app.remove("home/water-plants.md");
    app.wait_for("3 open");
    snapshot!("hand_made_data_2_changed_outside", app.state());

    // The next write action cleans up: names follow titles, the order lists every file.
    app.press("G");
    app.wait_for("▌   • Fix login bug");
    app.press("K");
    app.wait_for("• Fix login bug\n     • Review PR 42");
    snapshot!("hand_made_data_3_cleaned_up", app.state());

    app.quit();
}

/// Filter, help, hints and the preview: what only shows or narrows, without changing files.
#[test]
fn filter_and_help() {
    let mut app = Markoff::start_with(&[
        ("home/buy-milk.md", "# Buy milk\n"),
        (
            "home/call-mom.md",
            "# Call mom\n\nAsk about the milk frother.\n",
        ),
        ("work/review-pr.md", "# Review PR\n"),
    ]);
    app.wait_for("• Review PR");

    // Matches titles and bodies, <BS> corrects a typo, <CR> keeps the filter.
    app.press("/milx");
    app.wait_for("milx");
    app.press("<BS>k");
    app.wait_for("• Call mom …");
    app.press("<CR>");
    app.press("o");
    app.wait_for("clear the filter first");
    snapshot!("filter_and_help_1_filtered", app.state());

    // `Esc` clears it from the filter input.
    app.press("/<Esc>");
    app.wait_for("• Review PR");

    // Other keys do nothing while the help is open, it closes with ? or <Esc>.
    app.press("?");
    app.wait_for("toggle help");
    snapshot!("filter_and_help_2_help", app.state());
    app.press("D?");
    app.wait_for("• Buy milk");
    snapshot!("filter_and_help_3_closed", app.state());

    // The preview shows a task's body, `p` hides it and shows it again.
    app.press("1j");
    app.wait_for("│ Ask about the milk frother.");
    app.press("p");
    app.wait_for(&format!("{} ? toggle help", "─".repeat(66)));
    assert!(!app.screen().contains("frother"), "{}", app.screen());
    app.press("p");
    app.wait_for("│ Ask about the milk frother.");

    app.quit();
}

/// The archive: a page of its own with a heatmap of done tasks, `<Tab>` / `<S-Tab>` switch to
/// it. It moves with the date (today's row, the month names), so this one checks what holds on
/// any day instead of snapshots.
#[test]
fn archive() {
    let today = jiff::Zoned::now().date();
    let earlier = today.saturating_sub(jiff::Span::new().days(10));
    let archive = format!("# {earlier}\n\n## work\n\n### Old one\n\nold notes\n\n### Old two\n");
    let mut app = Markoff::start_with(&[
        ("work/fix-bug.md", "# Fix bug\n"),
        (&format!(".archive/{earlier}.md"), &archive),
    ]);
    app.wait_for("✓ 0 done");

    // Recent days one cell each, the cursor on the last day with tasks done and its tasks below,
    // newest first.
    app.press("<Tab>");
    app.wait_for("daily");
    app.wait_for("• Old two");
    app.wait_for("• Old one");
    assert!(
        app.screen().lines().any(|row| row.ends_with(" Mon")),
        "{}",
        app.screen()
    );

    // `<CR>` gives the keys to the list, its task shows its body next to it. `p` hides it, so
    // does `<Esc>` back to the heatmap.
    app.press("<CR>j");
    app.wait_for("│ old notes");
    app.press("p");
    app.wait_for(&format!("{} ? toggle help", "─".repeat(66)));
    assert!(!app.screen().contains("old notes"), "{}", app.screen());
    app.press("p");
    app.wait_for("│ old notes");
    app.press("<Esc>");
    app.wait_for(&format!("{} ? toggle help", "─".repeat(66)));

    // Todo keys do nothing on it, `<S-Tab>` goes back to the list.
    app.press("X<S-Tab>");
    app.wait_for("• Fix bug");
    app.wait_for("✓ 0 done");

    // The page stays where it was, the pages wrap around.
    app.press("jX");
    app.wait_for("✓ 1 done");
    app.press("<Tab>");
    app.wait_for("• Old two");
    app.press("<Tab>");
    app.wait_for("──[1]─ work");

    app.quit();
}
