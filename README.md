# markoff

`markdown` + `checkoff` = `markoff`

A TUI for tasks stored as plain markdown files. Quick actions in the TUI, writing in your `$EDITOR`.

## Goals

- Plain markdown, close to how you would write it by hand.
- Works without markoff: version it with git, edit it with anything.
- Fits into other markdown setups, like a personal wiki.
- Keyboard first, with vim-like keys.
- Short-term tasks only. Keeping the list small is the point, so features like filtering stay minimal.

## Install

Prebuilt static binary for Linux (x86_64): download `markoff-x86_64-unknown-linux-musl.tar.xz` from the latest [release](https://github.com/eckon/markoff/releases), then:

```sh
tar -xJf markoff-x86_64-unknown-linux-musl.tar.xz --strip-components=1 -C ~/.local/bin markoff-x86_64-unknown-linux-musl/markoff
```

Do the same again to update.

From source:

```sh
cargo install --path .
```

## Usage

Point markoff at a directory (first match wins):

```sh
markoff --dir ~/notes/tasks
MARKOFF_DIR=~/notes/tasks markoff
```

```toml
# ~/.config/markoff/config.toml
[general]
dir = "~/notes/tasks"
```

In an empty directory, create a section with `N`, then tasks with `o`. `?` lists all keys, `markoff --help` the arguments.

## Files

```text
<root>/
  .markoff             <- order, when repeating tasks were last done
  .archive/
    2026-10-14/        <- tasks done that day, laid out like the root
      work/
        old-task.md
  notes.md             <- ignored, files in the root are not tasks
  work/                <- section "work"
    fix-login-bug.md   <- task "# Fix login bug"
    review-pr.md
```

- A section is a directory, a task is a `.md` file in it starting with `# Title`.
- Filenames follow the title. Files added or edited by hand are renamed on start and before the next change.
- `.markoff` is written by markoff. New files go to the end, deleted ones are dropped. Add it to `.gitignore` to keep order changes and done dates out of git.
- Done tasks (`X`) move to `.archive/YYYY-MM-DD/<section>/` as they are. A task done twice a day gets a number: `call-bob-2.md`.
- The archive page (`Tab`) shows a heatmap of the done tasks back to the first archived day, a week per column. It scrolls sideways: the first column shows its month and year, below the heatmap the tasks done out of view are counted on each side. Brighter means more tasks per day, ranked in quarters of the days with anything done. The cursor in the heatmap picks a day, `v` a range of days like vim's visual mode. The app header shows the day with how long ago or the range, and how many tasks were done, the list below them by section, in a range each with its day, with the body of the selected task next to it while the list has the keys.

## Repeating tasks

A rule as inline code at the end of the heading makes a task repeat:

```markdown
# Water plants `* * mon`
```

The rule is the day part of cron: day of month, month, day of week. Both day fields have to match, `1 * mon` is a Monday the 1st.

| Rule          | Repeats                   |
| ------------- | ------------------------- |
| `* * *`       | Every day                 |
| `* * mon-fri` | Every weekday             |
| `* * thu`     | Every Thursday            |
| `1 * *`       | On the 1st of every month |
| `1 1 *`       | Every year on January 1st |

`X` on a repeating task keeps the task and notes the date in `.markoff`. The task hides until a day of its rule comes, a missed day stays until it is done. `.` shows the hidden ones. Code that is no rule (`` `cargo test` ``) stays part of the title.

## Keys

### Move

| Key         | Action                                      |
| ----------- | ------------------------------------------- |
| `j` / `k`   | Down / up, also `↓` / `↑`                   |
| `gg` / `G`  | Top / bottom, also `Home` / `End`           |
| `{` / `}`   | Previous / next section                     |
| `1`-`9`     | Jump to section, the number in `[1]`        |

### Change

| Key         | Action                                      |
| ----------- | ------------------------------------------- |
| `J` / `K`   | Move down / up, also `S-↓` / `S-↑`          |
| `<CR>`      | Open task in `$EDITOR`, or rename section   |
| `o` / `O`   | New task below / above                      |
| `N`         | New section                                 |
| `X`         | Done: archived, a repeating task hides      |
| `D`         | Delete task, or empty section               |
| `u`         | Undo                                        |

### Show

| Key         | Action                                      |
| ----------- | ------------------------------------------- |
| `p`         | Toggle the preview next to the list         |
| `.`         | Toggle hidden tasks: repeating, not due     |

### Global

On every page.

| Key         | Action                                      |
| ----------- | ------------------------------------------- |
| `/`         | Filter titles and bodies, `<CR>` keeps it   |
| `/` `Esc`   | Clear filter                                |
| `?`         | Toggle help, also closes with `Esc`         |
| `Tab`       | Next page: tasks, archive, `S-Tab` back     |
| `q` / `C-c` | Quit                                        |

### Archive page

| Key         | Action                                      |
| ----------- | ------------------------------------------- |
| `h` / `l`   | Heatmap left / right, also `←` / `→`        |
| `j` / `k`   | Heatmap a day down / up, list down / up     |
| `H` / `L`   | Like `h` / `l`, skips empty cells           |
| `J` / `K`   | Like `j` / `k` in the heatmap, skips empty  |
| `gg` / `G`  | Heatmap first day done / today              |
| `v`         | Select a range from here, `v` again ends it |
| `<CR>`      | Go to the tasks of the selected days        |
| `Esc`       | Back to the heatmap, there it ends a range  |
| `p`         | Toggle the preview, in the list             |
| `/`         | Filter, its own: the list keeps the matches |

In the list `gg` / `G`, `{` / `}` and `1`-`9` work like in the task list, the numbers count the sections shown. `1`-`9` in the heatmap jump into the list. With a filter, cells without a match turn gray and `H` / `J` / `K` / `L` skip them. The other keys of the tasks page do nothing on the archive page.

In the help, `j` / `k` and `gg` / `G` scroll it, `/` filters it by keys and texts until it closes, other keys do nothing.

In the filter, `C-w` deletes the last word. While a filter is set on the tasks page, moving and adding tasks is off.

The editor is `$VISUAL`, then `$EDITOR`, then `vi`.

## CLI

Commands for scripts, `--dir` works with them as usual:

| Command                 | Action                                                        |
| ----------------------- | ------------------------------------------------------------- |
| `list`                  | Print sections and tasks as markdown, in the order of the TUI |
| `add <section> <title>` | Add a task at the end of an existing section, print its path  |

`markoff list` prints:

```markdown
## work
- Fix login bug
- Review PR

## home
```

## Development

```sh
cargo test
cargo clippy --all-targets
cargo run -- --dir <scratch dir>
```

Design rules are in [AGENTS.md](AGENTS.md).
