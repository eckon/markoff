# markoff

> `markdown` + `checkoff` = `markoff`

A terminal task manager where every task is a markdown file

![markoff task page with sections, repeating tasks and the preview of the selected task](assets/tasks-view_050.png)

Tasks live in plain directories you can read, edit and version without markoff.
The TUI is for the quick things, writing happens in your editor.

> [!WARNING]
> markoff is experimental and still changing.
> Configuration, setups, keys and all in between could change between versions, without migration.
> Read the [changelog](CHANGELOG.md) before updating, or stay on a version that works for you.

## Why

- **Plain files.** A section is a directory, a task is a `.md` file. No database, no hidden metadata.
- **Your editor.** Writing happens in your `$EDITOR`, with your keys, plugins and spell check. markoff only does the quick actions around it: reorder, check off, undo.
- **Notes included.** A task is a whole file, so links, checklists and context fit next to the title.
- **Fits your setup.** Keep it in git, sync it, edit it with anything, put it in your wiki.
- **Keyboard first.** Vim-like keys, every action is one undo step.
- **Built to stay small.** For short-term tasks: done ones move to a dated archive, with a heatmap of what you got done.

## Install

Prebuilt binaries for Linux (x86_64, ARM) and macOS (Apple Silicon):

```sh
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/eckon/markoff/releases/latest/download/markoff-installer.sh | sh
```

Run it again to update. The binaries are also on the [releases](https://github.com/eckon/markoff/releases) page.

It installs a single binary to `~/.local/bin/markoff`, remove that file to uninstall.
Only if `~/.local/bin` was not on your `PATH`, it also adds an `env` script there and a line loading it to your shell config (like `~/.profile`, `~/.zshrc`).

From source:

```sh
cargo install --locked --git https://github.com/eckon/markoff
```

## Quick start

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

In an empty directory, press `N` to create a section, then `o` to create a task.
Both open your editor with a draft: write the title after `# `, save and quit. A task can take notes below its title.
`<CR>` opens a task in your editor again.

The editor is `$VISUAL`, then `$EDITOR`, then `vi`.
`?` lists all keys, `markoff --help` the arguments.

## Files

```text
<root>/
  .markoff             <- stores order and when repeating tasks were last done
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

## Archive

`Tab` opens the archive: a heatmap of every day you finished something, and the tasks of the selected day or range below it. Read-only, to answer "what did I do lately".

![markoff archive page with the heatmap, a selected range and its done tasks](assets/archive-view_050.png)

## Keys

All keys are also in the help inside markoff, `?` on any page.

<details>
<summary>Key reference</summary>

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

In the archive list `gg` / `G`, `{` / `}` and `1`-`9` work like in the task list. `1`-`9` in the heatmap jump into the list. With a filter, cells without a match turn gray and `H` / `J` / `K` / `L` skip them.

In the filter, `C-w` deletes the last word. While a filter is set on the tasks page, moving and adding tasks is off.

</details>

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

For the scratch dir, `./assets/make-example.py <dir>` builds an example root: sections, repeating tasks and a few months of archive. It is realistic data to try changes on without touching your own notes, and the data the screenshots come from.

Design rules are in [AGENTS.md](AGENTS.md).
