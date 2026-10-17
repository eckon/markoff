# Open tasks

Next steps for markoff. Remove a task once it is done, delete the file when nothing is left.

## Agreed

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
