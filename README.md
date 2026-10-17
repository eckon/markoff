# markoff

`markdown` + `checkoff` = `markoff`

TUI to manage todos in a human readable format (markdown) while allowing the use of any editor.

Every task is a markdown file, every section a directory. The order lives in `todo/.order`, so reordering never renames files. The TUI covers the quick actions (move, done, undo). Writing and editing happens in your `$EDITOR`.

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
  .order                    <- order of sections and tasks
  fix-typo.md               <- inbox (no section)
  work/                     <- section "work"
    fix-login-bug.md
    review-pr.md
archive/
  2026-10-14.md             <- tasks done that day
daily.md                    <- optional daily checklist
```

- A task file starts with `# Title`. The filename follows the title.
- `todo/.order` lists inbox tasks, then each section as `# name` with its tasks, one name per line (without `.md`). markoff writes it. Files it does not list go to the end of their section, names that no longer exist are dropped.
- To keep order changes out of git, add `todo/.order` to your `.gitignore`. The order then stays on each machine.
- Files created or edited by hand are renamed into this shape on start and with the next action. Renames on start are listed in a popup.
- Sections are created with `N` (or `mkdir todo/home`), renamed with `<CR>`, reordered with `J`/`K` and deleted with `D` when empty. A section's name is its directory name, from `N` and `<CR>` the heading as slug: `# Work Stuff` becomes `work-stuff`.
- Done tasks are appended to `archive/YYYY-MM-DD.md` under their section, then deleted from `todo/`.

### Daily checklist

If `daily.md` exists, its tasks show as section `daily` right below the inbox. It starts fresh every day:

```markdown
# Daily

## Book time in Jira [done 2026-10-17]

## Check the canteen menu

https://intranet/menu
```

- Each `## ` heading is a task, the text below it is shown in the preview.
- `X` marks a task done for today (struck through) or open again. It adds or removes the `[done <date>]` marker, nothing goes to the archive. An older date counts as open.
- The rule shows open/all tasks. Other keys do nothing here, edit `daily.md` by hand. A filter hides the section.

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
| `<CR>`        | Open task or rename section in `$EDITOR` |
| `n`           | New task, via `$EDITOR`               |
| `N`           | New section, via `$EDITOR`            |
| `X`           | Done: move to archive                 |
| `D`           | Delete task (without archiving) or empty section |
| `u`           | Undo                                  |
| `?`           | Help                                  |
| `q` / `C-c`   | Quit                                  |

The filter matches task titles and bodies, ignoring case, and updates while typing. Tasks listed only for a match in their body are dimmed and end in `…`. In its input `<CR>` keeps the filter, `Esc` clears it. A kept filter shows in the status line with the number of matching tasks. The right of the status line hints at the keys for the current mode. While a filter is set, moving and adding tasks are off.

The editor is `$VISUAL`, then `$EDITOR`, then `vi`.

## Development

```sh
cargo test
cargo clippy --all-targets
cargo run -- --dir <dir with todo/>
```

Design rules and conventions are in [AGENTS.md](AGENTS.md), details in the doc comments.
