# Idea: archive view

Built and through several feedback rounds, not final yet. Once final, its rules go to AGENTS.md,
README.md and the code (mostly done), and this file is removed.

## Why

A reminder of what was done recently, to continue a task that has several parts: "what did I do
yesterday", "what did I do for work in the last 3 weeks". Plus the overview the old archive popup
gave. Fully read-only: no writes from this page, not even opening a file in `$EDITOR`. Searching
the archive by text is not the goal, the editor does that.

## As it is now

### Pages

- Todo list and archive are pages, `[` / `]` switch and wrap around, so more pages can come
  later. The help stays an overlay.
- The header shows the keys around the pages, `── [ todo  archive ] ──`, the current one bold.
  On the archive page it shows the selected day or range instead of today, and only `✓ N done` of
  the selection.
- Each page keeps its state within the session (cursor, focus, `p`).
- `?` shows only the keys of the current page. On the archive page grouped by part: `heatmap`,
  `list`, `other`. `j` / `k` and `1`-`9` are in both parts.
- Todo keys (`X`, `D`, `o`, `u`, `/`, …) do nothing on the archive page, without a hint.

### Layout

```text
  heatmap: labels + 7 weekday rows, the selection blue, the cursor between brackets: ·[▓]·

  ════════════════════════╤════════════════════════
                          │
  ──[1]─ work ───── 2     │ ## Notes
 ▌   • Fix login  today   │ body of the task at the cursor
     • Review PR  Mon 13  │
```

- No frame. The part without the keys is dimmed, a double line with a blank line on each side
  between the two. No `Tab` hint, it was too noisy.
- One cursor on the page: the brackets in the heatmap or the bar in the list, only in the part
  with the keys. The selection is apart from it: the selected cell is blue in the heatmap, also
  while the list has the keys (dimmed with the rest). Today has no mark of its own, the page opens
  on the last day with tasks done. Visual mode can make more cells blue the same way.
- The heatmap keeps its height, the list gets the rest.

### Heatmap

- The cursor's cell is the selection, in blue: on the right a day, further back a cell covers
  more days.
- `h` / `l` a column, `j` / `k` a row, empty cells too. `H` / `J` / `K` / `L` go on that way to the
  next cell with tasks done. Both stop at the edges and before days after today.
- The cursor remembers a date: after a resize or a reload it lands on the cell that holds it.
- On first opening: the last day with tasks done, so on a Monday Friday. The heatmap has the keys.
- `1`-`9` jump into the list, to that section's first task, if it is shown.

### List

- The tasks of the selected cell, grouped by section in `.order` order, sections only in the
  archive after them by name. Newest first within a section.
- Section keys count the sections shown, `[1]`-`[9]`, the colors stay the todo list's.
- Each row: the title and the done day on the right edge, relative when near (`today`,
  `yesterday`, `Mon 13`), the date further back. The title wraps at the cursor.
- `j` / `k`, `gg` / `G`, `{` / `}`, `1`-`9` like in the todo list. Moving the heatmap cursor puts
  the list cursor on the first task.
- The task at the cursor shows its body in a preview on the right, like in the todo list, but only
  while the list has the keys. `p` hides or shows it, on each page of its own, shown by default.
  The body's headings are shifted back to how they were in the task.

### Code

- `app/archive.rs` the page's state and actions, `ui/archive/` its heatmap and list.
- Shared with the todo list: the cursor moves (`cursor.rs`), the list renderer (`ui/rows.rs`) and
  the list with its preview (`list_with_preview` in `ui.rs`). Rows are built per page, writing
  actions and what the cursor follows (file path or position) stay per page.

## Open points

1. **First open.** The list cursor can land on a section header when the last day with tasks is
   in a cell of several days: the heatmap's columns are only known after the first frame.
2. **Header length.** The page keys made the header longer, at 80 columns the todo preview's
   divider no longer joins it (`┬`), the date is in the way.
3. **Uppercase keys.** AGENTS.md says destructive keys are uppercase. On the read-only archive
   page `H` / `J` / `K` / `L` are not destructive, and `J` / `K` mean something else than on the
   todo page (move a task). Fine for a read-only page, but the rule in AGENTS.md should say so
   when this file goes.
4. **Body parsing.** A `# ` heading in a task body becomes `### ` in the archive, so it shows up
   as a task of its own and cuts the body there. It counted as a task before too. Fixing it means
   changing how tasks are archived.

## Later

Not in this version, the design leaves room for them:

- Visual mode in the heatmap: `v` anchors, `h` / `l` extend the selection over several cells, for
  "the last 3 weeks".
- Date words to pick a range, without an LLM: a fixed table, like git's `--since` or taskwarrior.

  | Input                                      | Meaning                                  |
  | ------------------------------------------ | ---------------------------------------- |
  | `today`, `yesterday`                       | that day                                 |
  | `week`, `last week`                        | this or last calendar week               |
  | `month`, `last month`, `year`, `last year` | the same                                 |
  | `mon` … `sun`                              | the last such day                        |
  | `3d`, `2w`, `1m`                           | the last 3 days, 2 weeks, 1 month        |
  | `2026-10-14`, `2026-10`, `2026`            | a day, month or year, prefix of the file |

  Pure logic next to `filter.rs`. Crates exist (`chrono-english`), but bring chrono and far more
  than needed. Key open.
- Narrowing to sections, shared with the todo page: the same logic for both.
- Every day selectable: now further back a cell covers several days, so old single days can't be
  picked. A heatmap with a cell per day that scrolls would allow it, but the view of all years at
  once should stay, it is what the heatmap is liked for. Ways to have both, to discuss:
  - Zoom: a key switches between the overview of all years and days that scroll, the cursor keeps
    its date.
  - Drill in: on a cell of several days a key shows its days, like a zoom on just that cell.
  - Scroll at the edge: the daily part moves back when the cursor leaves it on the left, the
    zones further back shrink. Keeps one view, but the overview changes while moving.

## Before it is final

- Tests. There are: unit tests for the pure parts (heatmap steps, bodies, relative days, cursor
  moves via the todo list), the list renderer, the archive preview with its joins, page switching
  and `p`, which the `filter_and_help` workflow covers on the todo page too. Missing: app tests
  for the heatmap moves, skipping, focus and section keys, render tests for the cursor brackets
  and small screens, and the archive part of the workflow scenarios (moving, `Tab`, the list, the
  preview).
- This file goes, what is left of it moves to AGENTS.md.

## After it

- A fixed date for tests: the TODO at `app::today()` in `src/app.rs`.
