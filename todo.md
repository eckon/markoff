# Open tasks

Next steps for markoff. Remove a task once it is done, delete the file when nothing is left.

## Agreed

- [ ] Key hints in the status line. Left: message, filter input or kept filter (drops its `Esc clears`). Right: dim hints for the current mode (normal / filter input / filter set), cut from the end when the left needs the room. Hint texts come from a short field in `BINDINGS`. Always on. Exact hints per mode still to pick.
- [ ] `N` new section below the cursor's section, via `$EDITOR` with the same draft as a rename. New sections go after the inbox.
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
