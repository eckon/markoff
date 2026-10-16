# Review 2026-10-16

Findings from a full code review, to work through before folding. Delete this file when done.

## Code

### Simplify

- [x] `edit.rs`: `Entry.name: Option<Name>` is only used to compare slugs, `current_slug: Option<String>` is enough
- [x] `edit.rs`: `target_names` takes `orders`, which is always `1..=n`, take the order from the position
- [x] `edit.rs`: `unique_slug(&[Name], ..)` only reads slugs, take `&[String]`
- [x] `edit.rs`: `renumber(..).map(|(r, _)| r).unwrap_or_default()` 3 times (delete, normalize, normalize_sections), one small helper
- [x] `edit::insert` returns the new path like `move_task`, then `created_path` in `app.rs` goes away
- [x] `app.rs`: "remember path, reload, clamp, select" 4 times (`reload_keep_selection`, `normalize`, `undo`, `apply` + `select_path`), one `reload_and_select(path)`
- [x] `app.rs`: `normalize_before_action()` in 4 match arms, group the writing actions
- [x] "section is full (99 tasks)" twice, build it once from `MAX_ORDER`

### Leftovers

- [x] Stale doc comments: `dd`/`d` in `keymap.rs` (label, `Keymap`, `pending`) and `ui.rs` (`render_status`), `NNN-` in `task.rs` and `edit.rs` (`Entry`)
- [x] Status "renamed N files to match their titles" is often wrong now (renames also fix the order). Decide the new text, e.g. "renamed N files (title/order)".
- [x] `op::write_atomic` and `keymap::HINTS` are `pub` but only used in their module
- [x] Help popup has a fixed width of 44, measure the content like the rename popup
- [x] `editor.rs`: `use tempfile` sits between the std and color_eyre imports
- [x] Trim PLAN.md to the open next steps. First move any "why" that is not in the code yet into doc comments.

### Left as is (on purpose)

- `after_create` does not reload after the editor: if the files changed meanwhile, the ops refuse and roll back, the status says "task not added". Rare, not worth more logic.
- `Section.path: Vec<String>` (for nesting), `order_renames` (guards hand-made duplicate slugs), hand-written `--dir` parsing, `Debounce`/`Fence`/`config`/`op`

## Setup

### CI (GitHub Actions)

- [ ] `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test` on push and PR
- [ ] Linux and macOS, `Swatinem/rust-cache`

### Rust config

- [ ] `rust-toolchain.toml`: pinned stable with `rustfmt` and `clippy`
- [ ] `Cargo.toml`: `rust-version` (let-chains need 1.88+), `description`, `license`, `repository`
- [ ] `[lints]`: `unsafe_code = "forbid"`, clippy `dbg_macro`, `todo`, `unwrap_used` (allowed in tests). Maybe `pedantic` as warn with a few allows.
- [ ] `rustfmt.toml`: defaults are fine. `group_imports` would fix the import order automatically but needs nightly.

### Dependencies and quality

- [ ] `cargo deny` or `cargo audit`
- [ ] Dependabot or Renovate
- [ ] `cargo machete` (all 7 deps are used today)
- [ ] `typos` for spelling
- [ ] Test that the README key table matches `BINDINGS`
- [ ] Optional: `cargo llvm-cov` coverage, `insta` snapshots for the UI

### Repo

- [ ] `LICENSE`
- [ ] `.gitignore`: `.rumdl_cache`
- [ ] `.rumdl.toml` with MD013 (line length) off or relaxed
- [ ] `.editorconfig`
- [ ] Pre-commit hook (lefthook or a git hook): fmt, clippy, test
- [ ] Releases: `cargo-dist` or GitHub releases with binaries

### AGENTS.md (one line each)

- [ ] Exact CI commands (`cargo clippy --all-targets -- -D warnings`)
- [ ] tmux recipe for TUI checks (`new-session -d -x 80 -y 20`, `send-keys`, `capture-pane`)
- [ ] Never run against the real notes dir
- [ ] A key change updates `BINDINGS`, the README table and the tests together
- [ ] Prefer deleting over keeping code for later
