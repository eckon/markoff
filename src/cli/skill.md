# markoff

These are the user's tasks, kept for the user. Read or change them only when the user asks. Never use markoff for your own plans, todos or notes, use your own tools for that.

The tasks are markdown files in a root directory. A section is a directory, a task is a `.md` file in it: the `# Heading` is its title, the rest its body (notes, checklists, links). Change them only with these commands, never the files: order and dates live in `.markoff`, done tasks move to `.archive`, filenames follow the title.

## Ids

A task's id is its path without `.md`: `work/fix-login-bug`. A section's id is its name: `work`. Take ids from `list`, never guess. A new title changes the id, so use the one a command prints.

## Commands

```sh
markoff list [<section>...] [--filter <query>] [--hidden]         # open tasks, `--hidden` adds repeating ones not due
markoff list --archive [--from <YYYY-MM-DD>] [--to <YYYY-MM-DD>]  # done and canceled tasks by day, default the last 7 days
markoff show <id>                                                 # a task's content, also an archived one
markoff add <section> '<title>' [--body <text>] [--before <id> | --after <id>]
markoff add --section <name>
markoff edit <id> [--title '<title>'] [--body <text> | --append <text>]
markoff edit <section> --name <name>
markoff move <id> [--to <section>] [--before <id> | --after <id> | --top | --bottom]
markoff done <id>...                                              # finished: archived
markoff cancel <id>...                                            # won't do: archived, heading struck through
markoff delete <id>...                                            # gone, not archived, sections only when empty
```

- Text with several lines: `-` reads it from stdin, `markoff add work 'Title' --body - <<'EOF'`.
- A repeating task has a rule as inline code at the end of its title: day of month, month, day of week, like cron: ``'Water plants `* * mon`'``, in single quotes, the shell runs backticks in double quotes. `done` hides it until its next day. `--title` replaces the whole heading, keep the rule to keep it repeating.
- Write titles and bodies in the user's language and style. Leave existing text as it is unless asked.
- Prefer `done` or `cancel` over `delete`. Ask before deleting, or before changing many tasks at once: there is no undo.
- With many tasks, read what is needed: `list <section>` or `list --filter <query>` over a full `list`.
- Errors say how to fix the call. `markoff <command> --help` shows all options.
