# AGENTS.md

markoff is a Rust TUI for todos stored as markdown files. This file is the source of truth for how markoff works and is built. [README.md](README.md) is for users.

## Design

- Data stays plain markdown, readable and editable without the tool. No database, no hidden metadata.
- A task is a `.md` file, a section is a directory in `todo/`, files directly in `todo/` are the inbox. One level only, hidden files are ignored.
- Order lives in the name, so `ls` shows the same list: `01-slug`. Always dense (`01`, `02`, ...), every action renumbers what it touches.
- The `# Heading` is the task's title, the filename follows it.
- Normalize (prefix, slug) runs at startup and before every write action, never on watcher reloads: renaming a file another editor has open breaks that editor's next save.
- Done tasks go to `archive/YYYY-MM-DD.md` under their section heading.
- Writing text happens in `$EDITOR`, the TUI only has quick actions. Keys are vim-like, destructive ones uppercase.
- Every action is one undo step, undo puts the cursor back where it was.

## Working style

- Work in small steps (one concern at a time). Before each change, say what will change and wait for an ok.
- Discuss every key mapping, user-visible text and behavior choice first. Add nothing that was not agreed (no extra flags, options, keys or helpers).
- Keep it simple. Remove code that becomes unused, unless there is a stated reason to keep it.
- Do not commit, the user reviews and commits.
- Try-outs and prototypes go straight into the working tree of this repo, the user resets them with git. Only if that clashes with uncommitted changes, use a branch. A git worktree is the last resort. Never in copies outside the repo.

## Checks after every change

```sh
cargo fmt
cargo test
cargo clippy --all-targets -- -D warnings   # same as CI
```

For TUI changes also run the real app in tmux against a scratch `todo/` dir (or a copy of real data, never the real notes): `tmux new-session -d -x 80 -y 20 'cargo run -- --dir <scratch>'`, then `send-keys` and `capture-pane -p`.

## Code

- Pure logic (`edit.rs`, `archive.rs`, `name.rs`) never touches the disk. All writes go through ops in `op.rs`, each with an exact inverse for undo.
- The disk is the source of truth: after a change, reload instead of patching state.
- `App::update(Action)` never touches the terminal, so it is testable.
- All keys live in `BINDINGS` (`keymap.rs`), the help overlay reads the same table. A test checks that the README key table matches.
- Errors go to the status line, they never end the app.
- Colors: only the 16 terminal colors, so the look follows the terminal theme.
- Comments say why, not what. Match the density and style of the surrounding code.

## Tests

- Pure logic: plain unit tests, ops compared as short strings (`describe`).
- App actions: on a `TempDir`, checking the files on disk, the cursor and the status.
- Rendering: ratatui `TestBackend`, checking the screen text.
- When behavior changes, update the expectations, keep the coverage.

## Docs

- Keep this file to the bare minimum. Details belong in the code and its doc comments.
- When a rule here changes, update it in the same change as the code. Same for README.md (keys, data layout).
