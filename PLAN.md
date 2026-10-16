# markoff design

Rewrite of markoff.nvim as a standalone TUI. The TUI covers the quick daily actions. Everything else opens in `$EDITOR`, the same way git hands off to the editor for commit messages.

Starts fresh, no migration from markoff.nvim.

## Status

v1 is implemented: everything in this document describes current behavior, except the sections [Known limitations](#known-limitations) and [Next steps](#next-steps).

## Goals

- Data stays plain markdown and readable without the tool
- Order and structure are visible with `ls`
- Vim keybindings, close to the current picker
- No database, almost no metadata

## Location

- The root directory holds `todo/` and `archive/`
- Resolved in order: `--dir` flag, `MARKOFF_DIR` env var, config file (`$XDG_CONFIG_HOME/markoff/config.toml`, default `~/.config/markoff/config.toml`)
- Config file: `dir = "~/notes"`. `~/` is expanded, a relative path is relative to the config file.
- The config file is only read when neither flag nor env var is set, so a broken config never blocks `--dir`
- Works from any cwd. `todo/` must exist, `archive/` is created on the first archive.

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

- A directory is a section. One level only, deeper directories are ignored.
- Files directly in `todo/` are the inbox. It is always shown first, even when empty, so there is always somewhere to add a task.
- Sections are modelled as a path (`Vec<String>`) in code, so nesting can be added later without changing the data or archive format.
- Sections are created, renamed and deleted in the shell (mkdir, mv). The TUI shows them, reorders them (`J/K`) and moves tasks between them.
- A file is a task. Only `.md` files count, hidden files and directories (`.git`, swap files, markoff's own temp files and drafts) are ignored.
- The numbered prefix sets the order, for both files and directories. Gaps (`010`, `020`) mean most moves only create one file.
- A new task gets an order between its neighbours: `+10` when appended, the middle otherwise (`010`/`020` -> `015`).
- Moving with `J/K` uses the same rule, for tasks and section directories: the moved one is renamed into the gap between its new neighbours, so usually only one file or directory is renamed. Its content is not touched.
- When there is no gap left, the whole section is renumbered with even gaps, leaving a free slot where the task goes. Renumber and insert are one undo step.
- Prefixes are 3 digits. Gaps of 10 fit 99 tasks per section. Above that, renumbering uses smaller gaps (5, 2, 1), so the hard limit is 999 tasks per section.
- Renames during renumbering are ordered so a target name is always free first (`009-x -> 010-x` waits until the old `010-x` moved on).
- Files without a valid prefix are shown at the end of their section, sorted by name, until normalizing gives them a prefix (see [Normalize](#normalize)).
- The section display name is the directory name without the prefix (`010-work` -> `work`).

## Task identity

- The `# Title` heading is the source of truth. The filename is derived from it.
- Only the first non-blank line counts as the heading, and only if it is level 1. An empty `# ` counts as no heading.
- The filename follows the heading through normalizing (see below), the prefix is kept.
- Slug: German umlauts written out (`Ä` -> `ae`, `ß` -> `ss`), everything else transliterated to ASCII, lowercase, `a-z0-9` and `-` only, collapse repeats, max 50 chars. Nothing left -> `untitled`.
- On a collision within a section, append `-2`, `-3`, ...
- A file without a heading uses its filename as the title (`010-call-bob.md` -> `call bob`). Its name is kept, it only gets a prefix if it has none.

### Normalize

Files created or edited by hand get the same shape as markoff's own (`edit::normalize`):

- A file without a valid prefix gets one after the last task of its section, `+10` each, in the order they are shown. If that does not fit into 999, the section is renumbered with even gaps.
- The slug follows the heading. Without heading the name stays (`My Notes.md` -> `020-My Notes.md`).
- A task never takes a slug another task already has, it gets `-2`, `-3`, ... instead
- Only files. Section directories without prefix stay as they are for now.
- Runs at startup, before every action that writes (`X`, `D`, `J/K`, inserting a new task) and after `<CR>`. Never on watcher reloads: renaming a file another editor has open would make its next save recreate the old name. Not before `u`, which reverts exactly what it recorded.
- Not on the undo stack, undoing it would only be redone by the next action. The cursor follows a renamed task.
- Renames at startup are listed in a popup (any key closes it), since names changing on start can be a surprise. After an action, the status line says `renamed N files to match their titles`.
- If a rename fails, all are rolled back, the error goes to the status line and the action still runs on the unchanged files.

## Task file

```markdown
# Fix login bug

free markdown body
```

- No frontmatter is used yet
- Open means the file exists in `todo/`
- Done means the task is moved into the archive
- Delete (`D`) removes the file without archiving. No record is kept apart from the undo stack, git outside the tool covers history.
- The archive file name gives the done date
- No due dates or tags

## Archive

One file per day, `archive/YYYY-MM-DD.md` (local date). Done tasks get added under their section heading.

```markdown
# 2026-10-14

## inbox

### Fix typo

## work

### Fix login bug

body of the task
```

- The section heading is the section path joined with `/` (`## work`, later `## private/garden`). Inbox tasks go under `## inbox`.
- The task becomes `### Title`, and headings in its body move down two levels, capped at `######`
- Lines in fenced code blocks (```` ``` ```` or `~~~`) are never treated as headings, neither when shifting nor when looking for a section
- If the section heading already exists in today's file, the task goes to the end of that section. Otherwise the section is added at the end of the file.
- The task file is deleted in the same step
- Archiving happens right away when a task is marked done (`X`)

## TUI

- List of sections and tasks on the left, inbox first. The cursor can also sit on a section header, so `p` and `o` work on empty sections.
- Preview pane on the right shows the task body as raw text, read only, heading lines in bold. It only appears when the selected task has a body (more than blank lines). On empty tasks and section headers the list takes the full width.

### Look

Picked from a few variants tried in the real terminal.

- No boxes. The list is separated from the preview by a gray `│` on the left of the preview.
- Each section has a color, rotating blue, magenta, green, yellow (inbox first). Only the 16 terminal colors, so the look follows the terminal theme.
- Section header: a gray rule with the name in the section color and the task count dim at the end (`── work ────── 2`). A blank line before each section except the first, which the cursor skips.
- Task: `•` bullet in the section color
- Cursor: a `▌` bar in the section color in a one-column gutter, a selected task's title in the section color
- The preview title is a rule in the same style (without count), on the same row as the first list row
- The list keeps two columns free on the right, so the rules end before the divider
- Rules keep the width of the split list even when the preview is hidden, so they don't jump while moving
- Status line at the bottom: errors in red, info (renames) in the normal color, or the pending key of an unfinished key sequence. Cleared on the next key.
- The TUI has no text input. Anything that edits text (create, rename, body) goes through `$EDITOR`, where the user has their own keybindings.
- Only one mode (normal) plus the help overlay and the startup rename popup. While one of them is open, any key closes it and does nothing else.
- A file watcher (`notify`) reloads on external changes, debounced (100ms of quiet). The cursor stays on the same task. Own writes also trigger a reload, which is harmless.
- The screen is only redrawn after a key, a resize, a reload or the editor.

### Keys

| Key            | Action                                                      |
| -------------- | ----------------------------------------------------------- |
| `j` / `Down`   | Cursor down                                                 |
| `k` / `Up`     | Cursor up                                                   |
| `J` / `S-Down` | Move the task or section down                               |
| `K` / `S-Up`   | Move the task or section up                                 |
| `<CR>`         | Open task in `$EDITOR`                                      |
| `o` / `O`      | New task below / above the cursor, via `$EDITOR`            |
| `X`            | Done: archive the task                                      |
| `D`            | Delete the task, without archiving                          |
| `u`            | Undo                                                        |
| `?`            | Toggle help overlay                                         |
| `q` / `C-c`    | Quit                                                        |

- On a section header, `o`/`O` both put the new task first in that section
- `J/K` on a task move it one place. At the edge of a section it goes into the neighbouring section, first or last place. On a section header they move the whole section. The inbox stays first. At the very top or bottom nothing happens. The cursor follows, each move is one undo step.
- `S-Down`/`S-Up` are Shift plus arrow keys. Shift only counts for non-letters, a shifted letter arrives as the uppercase key.
- The destructive actions are uppercase (`X`, `D`), so they are not hit by accident. Lowercase `x` and `d` only show a hint in the status line (`d does nothing, use D to delete`). The hints are a separate table (`HINTS`), not shown in the help.
- `D` on a section header is an error (`sections are deleted in the shell for now`). After a delete the status line says `deleted <title> (u to undo)`.
- `Esc` never quits.
- The keymap supports two-key sequences (`Seq`), currently no binding uses one. `Esc` or any other key cancels a pending one, `C-c` quits even halfway through.
- All keys live in one table (`BINDINGS` in `keymap.rs`). Key handling and the help overlay both read it, so they cannot disagree. A test presses every trigger in the table.

### Editor handoff

- Editor: `$VISUAL`, then `$EDITOR`, then `vi`. Run through `sh -c`, so values with arguments (`code --wait`) work. The path is passed as an argument, so spaces and quotes in names are safe.
- `<CR>`: the TUI suspends, the editor opens the task file, then the TUI resumes, reloads and syncs the filename to the heading. This is also how a task is renamed.
- `o/O`, git-commit style: the editor opens a draft containing `# `. The draft is a hidden file (`.markoff-new-XXXXXX.md`) in the section the task goes into, so relative `[](…)` and `[[…]]` links resolve while writing and stay valid for the task, which is created next to it. The loader ignores hidden files, so the draft never shows up in the list. It is deleted after saving or aborting. If markoff is killed while the editor is open, the hidden draft stays behind. If the saved file has a non-empty heading, the task is inserted at the cursor. An empty heading, no heading, or quitting without saving aborts with "aborted, empty title".
- For new tasks only, if the editor program is `vim` or `nvim` (also with a path or arguments), `+startinsert!` is added: insert mode at the end of `# `, like `A`. Plain `vi` is left out, it may be a vi without `startinsert`.

## Undo

- In-memory stack of applied op lists, cleared when the TUI exits. One user action is one entry, `u` applies its inverse.
- Ops: `Create`, `Remove`, `Rename`, `Update` (replace content). Each has an exact inverse, e.g. `Create` <-> `Remove` with the same content.
- Delete is `Remove`, create is `Create`, a move is `Rename` (each plus `Rename`s if renumbered), archive is `Create`/`Update` of the archive file plus `Remove` of the task.
- Archive writes the archive first, then removes the task. If the remove fails, the rollback takes the archive entry out again: a task is never in both places or in neither.
- Undo puts the cursor back where it was before the action: on the restored task after a delete or archive, on the row where `o`/`O` was pressed, on the task or section at its old place after a move. Each undo entry stores that path next to its ops.
- External changes do not clear the stack. An undo that would touch a file changed since fails with "changed on disk" and stays on the stack, so it can be retried.

## Writes

- All changes to disk go through ops (`op.rs`), applied in a list. If one fails, the ones already applied are rolled back.
- Content is written to a hidden temp file in the same directory, flushed, then renamed over the target. A crash leaves the old or the new file, never half of one.
- `Create` and `Rename` refuse to overwrite an existing file (`fs::rename` would silently replace it)
- `Remove` and `Update` refuse if the file no longer has the content markoff read, so edits made elsewhere are never lost
- Git stays outside the tool

## Code structure

| Module       | Role                                                                          |
| ------------ | ----------------------------------------------------------------------------- |
| `main.rs`    | Resolve root, load, run the TUI                                               |
| `config.rs`  | `--dir`, `MARKOFF_DIR`, config file                                           |
| `name.rs`    | `010-slug` parsing, `slugify`, `order_between`                                |
| `task.rs`    | `Task` and `Section`, heading/body split                                      |
| `load.rs`    | Read `todo/` into sections, sorted by prefix                                  |
| `op.rs`      | File ops with inverses, `apply_all` with rollback, atomic writes              |
| `edit.rs`    | Pure: turn delete, insert, move, archive and normalize into ops (incl. renumbering) |
| `archive.rs` | Pure: archive file text, heading shift, code fence handling                   |
| `keymap.rs`  | `BINDINGS` and `HINTS` tables, key -> `Action`, pending sequences             |
| `app.rs`     | State, run loop, `update(Action)`, undo stack                                 |
| `editor.rs`  | Suspend the TUI and run `$EDITOR`                                             |
| `watch.rs`   | File watcher with debounce                                                    |
| `ui.rs`      | Rendering: list, preview, status line, help                                   |

- The disk is the source of truth. After every change the app reloads `todo/` instead of patching its in-memory state.
- `edit.rs` and `archive.rs` decide what should happen without touching the disk, `op.rs` does it. Most logic is tested without a file system.
- `update(Action)` never touches the terminal. Editor work is requested via a field and done by the run loop, so `update` is testable without a terminal.
- Errors from actions end up in the status line, they never end the app

## Testing

- Pure logic (names, slugs, ordering, renumbering, archive text, keymap) with plain unit tests
- File ops and app actions on temp directories (`tempfile`), checking the files on disk
- Archive output compared against expected strings inline in the tests
- TUI rendering via ratatui's `TestBackend`, checking the screen text
- `cargo test` and `cargo clippy --all-targets` are clean

## Known limitations

- A task body with an unclosed code fence makes the rest of that archive file look like code. Later tasks for sections below it are added as a duplicate section at the end. Nothing is lost.
- If any task file cannot be read (permissions, not UTF-8), loading fails with an error naming that file
- The list creates its scroll state per frame. Scrolling works, but moving up in a long list scrolls a bit differently than expected. Fix: keep the `ListState` offset in `App`.
- A heading changed outside markoff only renames the file with the next action (or the next start)
- The editor runs through `sh`, so Windows is not supported
- No frontmatter support: a file starting with `---` is treated as having no heading

## Next steps

### Feedback round 1

From daily use (notes in `notes/todo/markoff`), refined in an interview. Work in this order, each step small and committed on its own. When a step is done, its behavior moves into the sections above and the step is removed here.

#### D. Navigation and structure (050, 060)

8. **Jumps.** `gg`/`G` for first/last row, `{`/`}` for previous/next section header. `gg` is the first binding using the sequence support again, so it brings back the sequence tests that were removed with `dd`.
9. **Fold sections.** `za` (and `<Tab>`) toggles the section under the cursor (or the one the cursor is in). Folded headers show their count. Fold state is per session, in memory. `J/K` over a folded section moves the task past the whole section.
10. **Focus one section.** A key (`f`, still free) shows only the section under the cursor. Same key again shows everything. Decide whether the inbox stays visible.
11. **`/` filter.** Typing filters the list live, fuzzy over titles, section names and task bodies. A matching section name shows all its tasks. A body match shows the matching line (in the preview or dim under the title). `<CR>` keeps the filter and the cursor on the match, `Esc` clears it. This is the first text input in the TUI, so it gets its own small input mode.

### Later

1. Fix the list scroll state (see limitations), noticeable once a list is longer than the screen. Best done together with folding/filter, which change the rows.
2. `markoff add` CLI command for quick capture from the shell
3. Redo `C-r`: the undo stack already stores inverses, redo is a second stack
4. Footer with key hints

Deferred, needs design first:

- Nested sections (data model and archive format already allow it, archive heading would be `## private/garden`)
- Section create, rename and delete in the TUI. `D` on a section header deletes it only if it is empty, otherwise an error says so. The inbox can never be deleted. Creating a section in the TUI belongs to the same feature.
- Normalize section directories too (prefix for a hand-made `mkdir todo/home`). Directory renames exist now (`J/K`), so this should be aligned with them.
- Recurring tasks. One option is a `recur: weekly` key in frontmatter. On archive, the tool creates a fresh copy instead of only deleting the task. Needs frontmatter support in `task.rs` first.
- More vim keys: counts, `C-d/C-u`, repeat `.`
