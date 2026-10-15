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

/// Leaves the TUI, opens `path` in the editor, and comes back once the editor exits.
pub fn open(terminal: &mut DefaultTerminal, path: &Path) -> Result<()> {
    ratatui::restore();
    let status = command(&editor(), path).status();

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

/// Opens the editor on a new temp file with an empty `# ` heading and returns what was saved.
///
/// The file lives in the system temp dir, not in `todo/`, so a half written task never shows
/// up in the list. The `.md` suffix gives editors markdown highlighting.
pub fn create(terminal: &mut DefaultTerminal) -> Result<String> {
    let file = tempfile::Builder::new()
        .prefix("markoff-new-")
        .suffix(".md")
        .tempfile()
        .wrap_err("failed to create temp file")?;
    fs::write(file.path(), "# \n")?;
    open(terminal, file.path())?;
    // Read by path: editors often replace the file instead of writing into it.
    Ok(fs::read_to_string(file.path())?)
}

/// `$VISUAL`, then `$EDITOR`, then `vi`, like git.
fn editor() -> String {
    ["VISUAL", "EDITOR"]
        .into_iter()
        .filter_map(|var| std::env::var(var).ok())
        .find(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "vi".to_string())
}

/// Runs through `sh` so editors with arguments (`code --wait`) work.
/// The path is passed as `$1`, never pasted into the script, so spaces or quotes are safe.
fn command(editor: &str, path: &Path) -> Command {
    let mut command = Command::new("sh");
    command
        .arg("-c")
        .arg(format!("{editor} \"$1\""))
        .arg("sh")
        .arg(path);
    command
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use super::*;

    #[test]
    fn command_passes_path_with_spaces_and_editor_args() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("a 'quoted' file.md");
        fs::write(&path, "# Old\n").unwrap();

        // An "editor" with arguments that changes the file.
        let status = command("sed -i s/Old/New/", &path).status().unwrap();
        assert!(status.success());
        assert_eq!(fs::read_to_string(&path).unwrap(), "# New\n");
    }
}
