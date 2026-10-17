# Open tasks

Next steps for markoff. Remove a task once it is done, delete the file when nothing is left.

## Later

- [ ] `markoff add` CLI command for quick capture from the shell
- [ ] Redo `C-r`: the undo stack already stores inverses, redo is a second stack
- [ ] Footer with key hints

## Deferred, needs design first

- [ ] Nested sections (data model and archive format already allow it, archive heading would be `## private/garden`)
- [ ] Section create, rename and delete in the TUI. `D` on a section header deletes it only if it is empty, otherwise an error says so. The inbox can never be deleted.
- [ ] Recurring tasks. One option is a `recur: weekly` key in frontmatter: on archive, a fresh copy is created instead of only deleting the task. Needs frontmatter support in `task.rs` first.
- [ ] More vim keys: counts, `C-d`/`C-u`, repeat `.`

Dropped: folding sections and focusing one section.

## Outside the code

- [ ] Turn on Dependabot alerts in the GitHub repo settings (replaces `cargo audit`)
- [ ] Check that the macOS CI job is green after the `sed -i.bak` test fix
- [ ] `rust-version = "1.89"` comes from the dependencies, nothing builds with 1.89 yet. Add a CI job if it matters.
- [ ] `rust-toolchain.toml` (1.99.0) is bumped by hand
