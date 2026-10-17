# markoff

`markdown` + `checkoff` = `markoff`

TUI to manage todos in a human readable format (markdown) while allowing the use of any editor.

Every task is a markdown file, every section a directory. The order lives in the filenames, so `ls` shows the same list as the TUI. The TUI covers the quick actions (move, done, undo). Writing and editing happens in your `$EDITOR`.

## Status

v1 works: list and preview, create and edit via `$EDITOR`, move tasks and sections, archive, delete, undo, filter, file watcher.

## Setup

```sh
cargo install --path .
```

Point markoff at a directory that contains a `todo/` directory, in one of these ways (first match wins):

```sh
markoff --dir ~/notes
MARKOFF_DIR=~/notes markoff
```

```toml
# ~/.config/markoff/config.toml (or $XDG_CONFIG_HOME/markoff/config.toml)
dir = "~/notes"
```

## Data layout

```text
todo/
  01-fix-typo.md           <- inbox (no section)
  01-work/                 <- section "work"
    01-fix-login-bug.md
    02-review-pr.md
archive/
  2026-10-14.md             <- tasks done that day
```

- A task file starts with `# Title`. The filename follows the title.
- The number prefix is the order. markoff picks and renumbers prefixes itself.
- Files created or edited by hand are renamed into this shape on start and with the next action. Renames on start are listed in a popup.
- Sections are created and renamed in the shell (`mkdir todo/03-home`), and reordered with `J`/`K`.
- Done tasks are appended to `archive/YYYY-MM-DD.md` under their section, then deleted from `todo/`.

## Keys

| Key           | Action                                |
| ------------- | ------------------------------------- |
| `j` / `↓`     | Cursor down                           |
| `k` / `↑`     | Cursor up                             |
| `gg` / `Home` | Go to top                             |
| `G` / `End`   | Go to bottom                          |
| `{` / `}`     | Previous / next section               |
| `/`           | Filter                                |
| `Esc`         | Clear the filter                      |
| `J` / `S-↓`   | Move task or section down             |
| `K` / `S-↑`   | Move task or section up               |
| `<CR>`        | Open task in `$EDITOR`                |
| `o` / `O`     | New task below / above, via `$EDITOR` |
| `X`           | Done: move to archive                 |
| `D`           | Delete task, without archiving        |
| `u`           | Undo                                  |
| `?`           | Help                                  |
| `q` / `C-c`   | Quit                                  |

The filter matches task titles and bodies, ignoring case, and updates while typing. Tasks listed only for a match in their body are dimmed and end in `…`. In its input `<CR>` keeps the filter, `Esc` clears it. A kept filter shows in the status line with the number of matching tasks. While a filter is set, moving and adding tasks are off.

The editor is `$VISUAL`, then `$EDITOR`, then `vi`.

## Development

```sh
cargo test
cargo clippy --all-targets
cargo run -- --dir <dir with todo/>
```

Design rules and conventions are in [AGENTS.md](AGENTS.md), details in the doc comments.
