# Idea: archive view

Built, waiting for the feedback round. Once final, its rules go to AGENTS.md, README.md and the
code (mostly done), and this file is removed.

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

## As built

Where the first version differs from the plan above or fills a gap:

- No frame (first feedback): the part without the keys is dimmed, the line between overview and
  list says where `Tab` moves them (`Tab list` / `Tab heatmap`).
- The header shows the page keys around the pages, `[ todo  archive ]`, and on the archive page
  the selected day or range instead of today. The status line has no page hint.
- In the list `h` / `l` do nothing, in the heatmap `gg` / `G`, `{` / `}` and `1`-`9` do nothing.
- Moving the heatmap cursor puts the list cursor on its first task, like a jump to a section.
- The list keeps at least 5 rows: below that the stats go, then the blank lines around the
  heatmap.
- Done tasks come from the `### ` headings of the archive files, so a `# ` heading in a task body
  shows up as a task of its own (it did count as one before too).
- The cursor moves of both lists are shared in `cursor.rs`.

## Open for the feedback round

- The reversed cursor cell is hard to see on an empty cell (`·`).
- Section keys in the list: the todo list's (`[3]` for the third todo section, also when only
  it has tasks) or counted in the list (`[1]`-`[9]` in the order shown).
- The header is longer now, at 80 columns the todo preview's divider no longer joins it (`┬`).
- On the first open the list cursor can land on a section header when the last day with tasks is
  in a cell of several days: the heatmap's columns are only known after the first frame.

## Before it is final

- Tests: while building only unit tests for pure logic were added where they helped. The missing
  app (archive actions, focus, page switching), render (list, focus, small screens) and workflow
  tests are added when finalizing, to keep the code at the usual standard.
- This file goes, what is left of it moves to AGENTS.md.
