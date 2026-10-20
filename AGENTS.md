# AGENTS.md

markoff is a Rust TUI for todos stored as markdown files. This file is the source of truth for how markoff works and is built. [README.md](README.md) is for users.

## Design

- Data stays plain markdown, readable and editable without the tool. No database, no hidden metadata in the tasks: markoff's own state is plain text in `.markoff`.
- The user picks the root directory, markoff adds no folder of its own. A task is a `.md` file, a section is a directory in the root. Files directly in the root are ignored, every task lives in a section. One level only, hidden files are ignored.
- Order lives in `.markoff` in the root, not in the filenames: renumbering prefixes renamed every file below a change, which cluttered git. The disk decides what exists, the file only orders it: unlisted files go to the end, stale names are dropped. It also keeps when a repeating task was last done, on the task's line, so checking one off never touches the task file and stays out of git with `.markoff`.
- The `# Heading` is the task's title, the filename follows it.
- Normalize (slug, cleaned `.markoff`) runs at startup and before every write action, never on watcher reloads: renaming a file another editor has open breaks that editor's next save.
- Done tasks go to `.archive/YYYY-MM-DD.md` in the root under their section heading. The header counts today's, so archiving is visible. The archive is a page of its own next to the todo list (`[` / `]`, the help stays an overlay): a heatmap back to the first day, cells growing with age so years fit, and the done tasks of the cell under its cursor. It answers "what did I do lately", searching stays with the editor. Read-only, not even `$EDITOR`. Each page keeps its state and has its own keys and help, keys of the other page do nothing.
- A task repeats with a rule as inline code at the end of its heading, ``# Water plants `* * mon` ``: the day fields of cron (day of month, month, day of week, both day fields have to match), read by croner. Nothing repeats more often than daily. `X` on it keeps the date in `.markoff` instead of archiving, the task hides until a day of the rule comes, a missed day stays due. `.` shows hidden tasks, like hidden files. Hidden tasks are not open, the filter and `list` skip them.
- Writing text happens in `$EDITOR`, the TUI only has quick actions. Keys are vim-like. Uppercase is a variant of its lowercase key, stronger or turned around (`O` / `o`, `H` / `h`), so the keys stay easy to guess.
- Every action is one undo step, undo puts the cursor back where it was.

## Config

`~/.config/markoff/config.toml`, parsed with serde into `Config` (`config.rs`), which mirrors the file. Its names are a public API, kept apart from code names. Keep new keys in line:

- `snake_case`. Global keys in `[general]`, everything else in a table per feature (`[archive]`), unfinished features in `[experimental]`.
- Every key is optional, with its default in the README.
- Unknown keys are an error, so typos and keys in the wrong table show.
- Types that can grow: a string over a bool when a third state is plausible, a table over a value that may get fields, units in the name or the value, lists always arrays.

## Working style

- Work in small steps (one concern at a time). Before each change, say what will change and wait for an ok.
- Discuss every key mapping, user-visible text and behavior choice first. Add nothing that was not agreed (no extra flags, options, keys or helpers).
- Keep it simple. Remove code that becomes unused, unless there is a stated reason to keep it.
- Keep files focused: when a feature grows its own part of a general file (like the archive page in `ui/archive.rs`), move it into its own module. Bring it up yourself when you notice it, don't wait to be asked.
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

- The archive page lives in `app/archive.rs` and `ui/archive/`. It shares the cursor moves (`cursor.rs`) and the list renderer (`ui/rows.rs`) with the todo list, so both lists behave alike.
- Pure logic (`edit.rs`, `archive.rs`, `cursor.rs`, `history.rs`, `name.rs`, `state.rs`) never touches the disk. All writes go through ops in `op.rs`, each with an exact inverse for undo.
- The disk is the source of truth: after a change, reload instead of patching state.
- `App::update(Action)` never touches the terminal, so it is testable.
- The date comes from `App.today`, read from the clock only in the main loop (and once for a CLI command), so a frame sees one day and tests can set it. Never call `Zoned::now()` elsewhere.
- All keys live in `BINDINGS` (`keymap.rs`), the help overlay reads the same table. Update the README key table with it.
- Errors go to the status line, they never end the app.
- Colors: only the 16 terminal colors, so the look follows the terminal theme. Colors carry meaning (sections, where you are, done, matches, errors), keys have none.
- Keys are bold in the default color, so they show without adding a color. Nothing else in the default color is bold.
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
