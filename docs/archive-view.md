# Idea: archive view

Agreed in discussion, not built yet. Once built, its rules go to AGENTS.md, README.md and the
code, and this file is removed.

## Why

A reminder of what was done recently, to continue a task that has several parts: "what did I do
yesterday", "what did I do for work in the last 3 weeks". Plus the overview the archive popup gives
today. Fully read-only: no writes from this page, not even opening a file in `$EDITOR`. Searching
the archive by text is not the goal, the editor does that.

## Pages

- The archive becomes a full page instead of the popup over the todo list. Help stays an overlay.
- `[` / `]` switch between the pages and wrap around, so more pages can come later. `a` is
  removed.
- The header shows the pages, `todo  archive`, the current one highlighted.
- Each page keeps its state within the session: the archive page keeps its cursor and focus when
  switching away and back.
- `?` shows only the keys of the current page.
- Todo keys (`X`, `D`, `o`, `u`, `/`, …) do nothing on the archive page, without a hint.

## Layout

```text
┌ archive ───────────────────────────────┐
│ heatmap: labels + 7 weekday rows       │
│ stats: period table, years, share bars │
├ Fri 2026-10-16 · 3 tasks ──────────────┤
│ work                                   │
│   Fix login bug              yesterday │
│   Review PR                     Mon 13 │
│ home                                   │
│   Call plumber               yesterday │
└────────────────────────────────────────┘
```

- Overview on top, as the popup has it now: heatmap and stats. The stats stay over all time, they
  do not follow the selection.
- Below it the done tasks of the selection.
- Small screens: the stats are dropped first. The heatmap keeps its height (labels + 7 rows), its
  width adapts as now. The list gets the rest and scrolls.

## Heatmap

- A cursor on a cell, the cell is the selection: on the right a day, further back a cell covers
  more days, so older selections are longer stretches.
- `h` / `l` a column, `j` / `k` a row. Stops at the edges, skips the days after today.
- The selected cell is drawn reversed.
- The cursor remembers a date: after a resize or a reload it lands on the cell that contains it.
- On opening: the last day with done tasks, today if something is done today, so on a Monday
  Friday. Heatmap focused.

## List

- The tasks of the selected cell, grouped by section like the todo list: sections in `.order`
  order, sections no longer in the todo list after them like any other section, under their
  archived name. Newest first within a section.
- Each row: the title, the done date on the right. Relative when near (`today`, `yesterday`,
  `Mon 13` within about a week), the full date further back (`2026-03-04`).
- The frame title says the range and the count: `Fri 2026-10-16 · 3 tasks`,
  `2026-03-02 – 2026-03-15 · 12 tasks`. An empty selection says so in the list.
- A cursor row like in the todo list: `j` / `k`, `gg` / `G`, `{` / `}`, `1`-`9`.

## Focus

- `Tab` switches between heatmap and list. The focused one has a highlighted frame, the other's
  cursor is dim.

## Later

Not in the first version, kept here so the design leaves room for them:

- `<CR>` on a done task shows its body, read-only (overlay or `$PAGER`, open).
- Visual mode in the heatmap: `v` anchors, `h` / `l` extend the selection over several cells.
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

## Notes for building

- The archive is loaded as counts per day and section now. The list needs the titles too.
- `ui/archive.rs` is already about 800 lines: split into heatmap, stats and list modules.
- The popup state (`show_archive`) becomes the current page.
- Not experimental, so the workflow tests cover it: the archive part of the scenario it is in
  changes from the popup to the page.
- README: keys table and the archive description.
- Tests while building: only unit tests for pure logic that help not to break things. The missing
  app, render and workflow tests are added when finalizing, to keep the code at the usual standard.
