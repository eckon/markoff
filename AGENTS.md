# AGENTS.md

markoff is a Rust TUI for todos stored as markdown files. This file is the source of truth for how markoff works and is built. [README.md](README.md) is for users.

## Design

- Data stays plain markdown, readable and editable without the tool. No database, no hidden metadata.
- The user picks the root directory, markoff adds no folder of its own. A task is a `.md` file, a section is a directory in the root. Files directly in the root are ignored, every task lives in a section. One level only, hidden files are ignored.
- Order lives in `.order` in the root, not in the filenames: renumbering prefixes renamed every file below a change, which cluttered git. The disk decides what exists, the file only orders it: unlisted files go to the end, stale names are dropped.
- The `# Heading` is the task's title, the filename follows it.
- Normalize (slug, cleaned `.order`) runs at startup and before every write action, never on watcher reloads: renaming a file another editor has open breaks that editor's next save.
- Done tasks go to `.archive/YYYY-MM-DD.md` in the root under their section heading. The header counts today's, so archiving is visible. The archive is a page of its own next to the todo list (`[` / `]`, the help stays an overlay): a heatmap back to the first day, cells growing with age so years fit.
- The daily checklist is an add-on in its own modules (`daily.rs`, `app/daily.rs`, `ui/daily.rs`), only with `[experimental] daily` in the config: `X` marks a task done for today. Kept apart until recurring tasks get a real design.
- Writing text happens in `$EDITOR`, the TUI only has quick actions. Keys are vim-like, destructive ones uppercase.
- Every action is one undo step, undo puts the cursor back where it was.

## Working style

- Work in small steps (one concern at a time). Before each change, say what will change and wait for an ok.
- Discuss every key mapping, user-visible text and behavior choice first. Add nothing that was not agreed (no extra flags, options, keys or helpers).
- Keep it simple. Remove code that becomes unused, unless there is a stated reason to keep it.
- Keep files focused: when a feature grows its own part of a general file (like the archive view in `ui/archive.rs`), move it into its own module. Bring it up yourself when you notice it, don't wait to be asked.
- Do not commit, the user reviews and commits.
- Try-outs and prototypes go straight into the working tree of this repo, the user resets them with git. Only if that clashes with uncommitted changes, use a branch. A git worktree is the last resort. Never in copies outside the repo.

## Checks after every change

```sh
cargo fmt
cargo test
cargo clippy --all-targets -- -D warnings   # same as CI
```

For TUI changes also run the real app in tmux against a scratch root dir (or a copy of real data, never the real notes): `tmux new-session -d -x 80 -y 20 'cargo run -- --dir <scratch>'`, then `send-keys` and `capture-pane -p`.

## Code

- Pure logic (`edit.rs`, `archive.rs`, `history.rs`, `name.rs`, `order.rs`) never touches the disk. All writes go through ops in `op.rs`, each with an exact inverse for undo.
- The disk is the source of truth: after a change, reload instead of patching state.
- `App::update(Action)` never touches the terminal, so it is testable.
- All keys live in `BINDINGS` (`keymap.rs`), the help overlay reads the same table. Update the README key table with it.
- Errors go to the status line, they never end the app.
- Colors: only the 16 terminal colors, so the look follows the terminal theme.
- Comments say why, not what. Match the density and style of the surrounding code.
- No helper function for a single use or a plain call chain, inline it unless it has a reason of its own.
- `.github/workflows/release.yml` is generated: change `dist-workspace.toml`, then run `dist generate` (`cargo install cargo-dist --locked`). To update dist, bump `cargo-dist-version` and regenerate.

## Tests

- Pure logic: plain unit tests, ops compared as short strings (`describe`).
- App actions: on a `TempDir`, checking the files on disk, the cursor and the status.
- Rendering: ratatui `TestBackend`, checking the screen text.
- Workflows (`tests/workflow.rs`): the real binary in a pty, a few long scenarios from start to quit, snapshots of screen and files at checkpoints. Keep them few and long: a new or changed feature goes into the scenario it fits, a removed one comes out of it, no scenario per feature. Experimental features stay out. A failing snapshot leaves a `.snap.new` (ignored by git): review it with `cargo insta review` (needs `cargo install cargo-insta --locked` once), or regenerate all with `INSTA_UPDATE=always cargo test --test workflow` and read the git diff. Never accept them unread.
- When behavior changes, update the expectations, keep the coverage.

## Docs

- Keep this file to the bare minimum. Details belong in the code and its doc comments.
- When a rule here changes, update it in the same change as the code. Same for README.md (keys, data layout).
