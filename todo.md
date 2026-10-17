# Open tasks

Next steps for markoff. Remove a task once it is done, delete the file when nothing is left.

## Agreed

- [ ] Key hints in the status line. Left: message, filter input or kept filter (drops its `Esc clears`). Right: dim hints for the current mode (normal / filter input / filter set), cut from the end when the left needs the room. Hint texts come from a short field in `BINDINGS`. Always on. Exact hints per mode still to pick.
- [ ] Sections feel like tasks:
  - `n` new task at the end of the cursor's section, cursor on it. Replaces `o`/`O` (removed).
  - `N` new section below the cursor's section, via `$EDITOR` like a task.
  - `<CR>` on a header renames it via `$EDITOR`. Draft is `# name` plus a git style comment that only the heading is used. Text below is ignored, the status says so.
  - Name clash with an existing section is an error.
  - `D` deletes an empty section, otherwise an error. The inbox can't be renamed or deleted, new sections go after it.
  - `X` on a header does nothing.
- [ ] Rework the help overlay: keys now mean the same on tasks and sections, notes per key would clutter it.
- [ ] Daily checklist, kept apart as an add-on (own module) until it gets a real design:
  - `daily.md` next to `todo/` and `archive/`, each `## ` heading is a task, text under it its body.
  - Shown as section `daily` right below the inbox.
  - `X` marks it done for today: `## Book time in Jira [done 2026-10-17]`. `X` again unmarks. Undo works. Nothing goes to the archive.
  - Done today shows dim, an old date counts as open.
  - No other keys, the file is edited by hand.

## Later

- [ ] `markoff add` CLI command for quick capture from the shell

## Deferred

- [ ] More vim keys: counts, `C-d`/`C-u`, repeat `.`

Dropped: folding sections, focusing one section, nested sections, redo, real recurring tasks (for now the daily checklist).
