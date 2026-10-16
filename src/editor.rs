//! Hands the terminal to `$EDITOR`, the same way git does for commit messages.

use std::{fs, io, path::Path, process::Command};

use color_eyre::{
    Result,
    eyre::{WrapErr, bail},
};
use ratatui::{
    DefaultTerminal,
    crossterm::{
        execute,
        terminal::{EnterAlternateScreen, enable_raw_mode},
    },
};
use tempfile::NamedTempFile;

/// Leaves the TUI, opens `path` in the editor, and comes back once the editor exits.
/// `args` go to the editor before the path.
pub fn open(terminal: &mut DefaultTerminal, path: &Path, args: &[&str]) -> Result<()> {
    ratatui::restore();
    let status = command(&editor(), args, path).status();

    // Back into the TUI even if the editor failed, otherwise the terminal stays broken.
    // Not `ratatui::init()`, that would install its panic hook a second time.
    enable_raw_mode()?;
    execute!(io::stdout(), EnterAlternateScreen)?;
    terminal.clear()?;

    let status = status.wrap_err("failed to start the editor")?;
    if !status.success() {
        bail!("editor exited with {status}");
    }
    Ok(())
}

/// Opens the editor on a new draft in `dir` and returns what was saved.
///
/// `dir` is the section the task will end up in, so relative and wiki links already resolve
/// while writing and stay valid when the task is created next to the draft.
pub fn create(terminal: &mut DefaultTerminal, dir: &Path) -> Result<String> {
    let file = draft(dir)?;
    open(terminal, file.path(), new_task_args(&editor()))?;
    // Read by path: editors often replace the file instead of writing into it.
    Ok(fs::read_to_string(file.path())?)
}

/// Hidden draft with an empty `# ` heading, deleted when dropped.
///
/// Hidden, so the loader ignores it and a half written task never shows up in the list.
/// The `.md` suffix gives editors markdown highlighting.
fn draft(dir: &Path) -> Result<NamedTempFile> {
    let file = tempfile::Builder::new()
        .prefix(".markoff-new-")
        .suffix(".md")
        .tempfile_in(dir)
        .wrap_err_with(|| format!("failed to create draft in {}", dir.display()))?;
    fs::write(file.path(), "# \n")?;
    Ok(file)
}

/// `$VISUAL`, then `$EDITOR`, then `vi`, like git.
fn editor() -> String {
    ["VISUAL", "EDITOR"]
        .into_iter()
        .filter_map(|var| std::env::var(var).ok())
        .find(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "vi".to_string())
}

/// Editor specific arguments for a new task, so the title can be typed right away.
/// The only place that knows about specific editors.
fn new_task_args(editor: &str) -> &'static [&'static str] {
    match program_name(editor) {
        // Insert mode at the end of the first line, like `A`: right after `# `.
        // Not plain `vi`, which may be a vi that does not know `startinsert`.
        "vim" | "nvim" => &["+startinsert!"],
        _ => &[],
    }
}

/// Program name of an editor command: `/usr/bin/nvim -u foo` -> `nvim`.
fn program_name(editor: &str) -> &str {
    let program = editor.split_whitespace().next().unwrap_or_default();
    Path::new(program)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
}

/// Runs through `sh` so editors with arguments (`code --wait`) work.
/// `args` and the path are passed as `"$@"`, never pasted into the script, so spaces or
/// quotes are safe.
fn command(editor: &str, args: &[&str], path: &Path) -> Command {
    let mut command = Command::new("sh");
    command
        .arg("-c")
        .arg(format!("{editor} \"$@\""))
        .arg("sh")
        .args(args)
        .arg(path);
    command
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use super::*;

    #[test]
    fn command_passes_args_and_path_with_spaces() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("a 'quoted' file.md");
        fs::write(&path, "# Old\n").unwrap();

        // An "editor" with its own argument (`-i`), plus an extra argument (the script).
        let status = command("sed -i", &["s/Old/New/"], &path).status().unwrap();
        assert!(status.success());
        assert_eq!(fs::read_to_string(&path).unwrap(), "# New\n");
    }

    #[test]
    fn new_task_args_by_program_name() {
        for editor in ["vim", "nvim", "/usr/bin/nvim", "nvim -u foo"] {
            assert_eq!(new_task_args(editor), ["+startinsert!"], "{editor}");
        }
        for editor in ["vi", "nvim-qt", "code --wait", "hx", ""] {
            assert!(new_task_args(editor).is_empty(), "{editor}");
        }
    }

    #[test]
    fn draft_is_hidden_in_dir_and_removed_on_drop() {
        let dir = TempDir::new().unwrap();
        let file = draft(dir.path()).unwrap();
        let path = file.path().to_path_buf();

        assert_eq!(path.parent(), Some(dir.path()));
        let name = path.file_name().unwrap().to_str().unwrap();
        assert!(
            name.starts_with(".markoff-new-") && name.ends_with(".md"),
            "{name}"
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), "# \n");

        drop(file);
        assert!(!path.exists());
    }
}
