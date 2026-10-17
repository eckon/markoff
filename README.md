# markoff

`markdown` + `checkoff` = `markoff`

A TUI for todos stored as plain markdown files. Quick actions in the TUI, writing in your `$EDITOR`.

## Goals

- Plain markdown, close to how you would write it by hand.
- Works without markoff: version it with git, edit it with anything.
- Fits into other markdown setups, like a personal wiki.
- Short-term todos only. Keeping the list small is the point, so features like filtering stay minimal.

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
markoff --dir ~/notes/todo
MARKOFF_DIR=~/notes/todo markoff
```

```toml
# ~/.config/markoff/config.toml
dir = "~/notes/todo"
```

In an empty directory, create a section with `N`, then tasks with `o`. `?` lists all keys, `markoff --help` the arguments.

## Files

```text
<root>/
  .order               <- order of sections and tasks
  .archive/
    2026-10-14.md      <- tasks done that day
  notes.md             <- ignored, files in the root are not tasks
  work/                <- section "work"
    fix-login-bug.md   <- task "# Fix login bug"
    review-pr.md
```

- A section is a directory, a task is a `.md` file in it starting with `# Title`.
- Filenames follow the title. Files added or edited by hand are renamed on start and before the next change.
- `.order` is written by markoff. New files go to the end, deleted ones are dropped. Add it to `.gitignore` to keep order changes out of git.
- Done tasks (`X`) are appended to `.archive/YYYY-MM-DD.md` under their section.

## Keys

### Move

| Key         | Action                                    |
| ----------- | ----------------------------------------- |
| `j` / `k`   | Down / up, also `↓` / `↑`                 |
| `gg` / `G`  | Top / bottom, also `Home` / `End`         |
| `{` / `}`   | Previous / next section                   |
| `1`-`9`     | Jump to section, the number in `[1]`      |

### Change

| Key         | Action                                    |
| ----------- | ----------------------------------------- |
| `J` / `K`   | Move down / up, also `S-↓` / `S-↑`        |
| `<CR>`      | Open task in `$EDITOR`, or rename section |
| `o` / `O`   | New task below / above                    |
| `N`         | New section                               |
| `X`         | Done, moves the task to the archive       |
| `D`         | Delete task, or empty section             |
| `u`         | Undo                                      |

### Other

| Key         | Action                                    |
| ----------- | ----------------------------------------- |
| `/`         | Filter titles and bodies, `<CR>` keeps it |
| `Esc`       | Clear filter, close help                  |
| `?`         | Toggle help                               |
| `q` / `C-c` | Quit                                      |

While a filter is set, moving and adding tasks is off.

The editor is `$VISUAL`, then `$EDITOR`, then `vi`.

## Daily checklist (experimental)

A checklist that starts fresh every day, shown above the sections:

```toml
[experimental]
daily = "daily.md"   # in the root
```

```markdown
# Daily

## Book time in Jira [done 2026-10-17]

## Check the canteen menu

https://intranet/menu
```

Each `## ` heading is a task. `X` marks it done for today by adding the `[done <date>]` marker, nothing is archived.

## Development

```sh
cargo test
cargo clippy --all-targets
cargo run -- --dir <scratch dir>
```

Design rules are in [AGENTS.md](AGENTS.md).
