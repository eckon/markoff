# markoff tui plan

Rewrite of markoff.nvim as a standalone TUI. The TUI covers the quick daily actions. Everything else opens in `$EDITOR`, the same way git hands off to the editor for commit messages.

Starts fresh, no migration from markoff.nvim.

## Goals

- Data stays plain markdown and readable without the tool
- Order and structure are visible with `ls`
- Vim keybindings, close to the current picker
- No database, almost no metadata

## Location

- The root directory holds `todo/` and `archive/`
- Resolved in order: `--dir` flag, `MARKOFF_DIR` env var, config file (`~/.config/markoff/config.toml`)
- Works from any cwd

## Data layout

```
todo/
  010-fix-typo.md           <- inbox (no section)
  010-work/
    010-fix-login-bug.md
    020-review-pr.md
  020-private/
    010-call-bob.md
archive/
  2026-10-14.md
```

- A directory is a section. v1 supports one level only.
- Files directly in `todo/` are the inbox. It shows at the top of the list.
- Sections are modelled as a path (`Vec<String>`) in code, so nesting can be added later without changing the data or archive format.
- Sections are managed in the shell (mkdir, mv) for v1. The TUI shows them and moves tasks between them.
- A file is a task.
- The numbered prefix sets the order, for both files and directories. Use gaps (`010`, `020`) so most moves rename one file.
- When there is no gap left, the tool renumbers the whole section back to `010, 020, 030, ...`. This is one undo step.
- Prefixes are 3 digits. Gaps of 10 fit 99 tasks per section. Above that, renumbering uses smaller gaps (5, 2, 1), so the hard limit is 999 tasks per section.
- Files without a valid prefix are shown at the end of their section. The first move gives them a prefix.
- The section display name is the directory name without the prefix (`010-work` -> `work`).

## Task identity

- The `# Title` heading is the source of truth. The filename is derived from it.
- On load and after the editor returns, if the filename slug doesn't match the heading, the tool renames the file and keeps the prefix.
- Slug: transliterate to ASCII (`Ä` -> `ae`), lowercase, `a-z0-9` and `-` only, collapse repeats, max ~50 chars.
- On a collision within a section, append `-2`, `-3`, ...
- A file without a heading uses its filename as the title and is not renamed.

## Task file

```markdown
# Fix login bug

free markdown body
```

- Frontmatter stays close to empty
- Open means the file exists in `todo/`
- Done means the task is moved into the archive
- Delete removes the file, with no record. Git outside the tool covers history.
- The archive file name gives the done date
- No due dates or tags for now

## Archive

One file per day, `archive/YYYY-MM-DD.md`. Done tasks get appended under their section heading.

```markdown
# 2026-10-14

## inbox

### Fix typo

## work

### Fix login bug

body of the task
```

- The section heading is the section path joined with `/` (`## work`, later `## private/garden`). Inbox tasks go under `## inbox`.
- The tool shifts the task's headings down by two levels so they fit under the section heading
- If the section heading already exists in today's file, the task is appended to the end of that section. Otherwise the heading is added at the end of the file.
- The tool deletes the task file after appending
- Archiving happens right away when a task is marked done

## TUI

- List of sections and tasks, same layout as the current picker. Inbox first.
- Preview pane shows the task body as raw text, read only. Light styling for headings at most.
- The TUI has no text input. Anything that edits text (create, rename, body) goes through `$EDITOR`, where the user has their own keybindings.
- Only one mode (normal) plus the help overlay
- A file watcher (`notify`) reloads on external changes. The tool ignores events caused by its own writes.

### Keys

| Key            | Action                                                       |
| -------------- | ------------------------------------------------------------ |
| `j` / `Down`   | Cursor down                                                  |
| `k` / `Up`     | Cursor up                                                    |
| `<CR>`         | Open task in `$EDITOR`                                       |
| `o` / `O`      | New task below / above the cursor, via `$EDITOR`             |
| `x`            | Done: archive the task                                       |
| `dd`           | Cut the task into the register                               |
| `p` / `P`      | Paste the register below / above the cursor, in any section  |
| `u`            | Undo                                                         |
| `?`            | Toggle help overlay                                          |
| `Esc`          | Cancel pending key or close help. Never quits.               |
| `q` / `C-c`    | Quit                                                         |

- `dd` is the only multi-key sequence in v1. `keymap.rs` keeps a small pending-key state for it, which can later grow into counts and `gg`.
- The keymap is a table, so the help overlay is generated from it.

### Editor handoff

- `<CR>`: the TUI suspends, `$EDITOR` opens the task file, then the TUI resumes, reloads and syncs the filename to the heading. This is also how a task is renamed.
- `o/O`, git-commit style: the tool opens `$EDITOR` on a temp file containing `# `. If the saved file has a non-empty heading, the task is created at that position. An empty heading, or quitting without saving, aborts.

### Register

- `dd` deletes the file and keeps its content (and old path, for undo) in an in-memory register. `p/P` write it into the target section. Cut without paste is a delete.
- Known tradeoff: a crash or quit while something is in the register loses that task.

## Undo

- In-memory log of operations and their reverse, cleared when the TUI exits
- Create, rename, move, delete and renumber are file ops with a clear reverse
- Archive is two ops: create or update today's archive file (the update keeps the content before and after), then remove the task. Undo restores the old archive content and the task file. A task can land in the middle of the archive file, so truncating would not work.
- Ops that remove or overwrite a file check its content first and refuse if it changed on disk.
- If an external change touches a file referenced by the log, the affected entries are dropped instead of risking a wrong undo

## Writes

- Write to a temp file, then rename it over the target
- Git stays outside the tool for now

## Language

Rust + ratatui. Enums model modes, pending keys and undo actions well.

## Testing

- Fixture directories with the expected tree after each operation
- Archive output compared against golden files
- TUI tests via ratatui's `TestBackend`

## Deferred (not v1)

- Nested sections (data model and archive format already allow it)
- Section create, rename, reorder and delete in the TUI
- Filter or fuzzy find with `/`. Either in the TUI, or the tool writes all tasks into a temp file and opens it in the editor.
- `markoff add` CLI command
- More vim keys: `gg/G`, counts, section jumps, `C-d/C-u`, redo `C-r`, repeat `.`
- `J/K` to reorder without the register (and Shift+arrows for non-vim users)
- Footer with key hints
- Recurring tasks. One option is a `recur: weekly` key in frontmatter. On archive, the tool creates a fresh copy instead of only deleting the task.
