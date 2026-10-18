# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.3.0](https://github.com/eckon/markoff/compare/v0.2.0...v0.3.0) - 2026-10-18

### Added

- *(tui)* allow jumping to prev/next section with c-u/c-d
- *(tui)* add c-w binding to delete word in filter mode
- *(cli)* add list command
- *(cli)* add add command
- *(cli)* align error messages

## [0.2.0](https://github.com/eckon/markoff/compare/v0.1.0...v0.2.0) - 2026-10-17

### Added

- add section stats in archive view
- add daily, monthly and yearly stats for the archive view
- improve archive view
- add archive history chart on the bottom
- add quick jump with 1-9 to sections
- handle basic cli flags and return readable errors

## [0.1.0](https://github.com/eckon/markoff/releases/tag/v0.1.0) - 2026-10-17

### Added

- remove readme tests and align it
- align status messages
- close app with `q` or `C-c` independant on the current open window
- move help window to the side panel
- group keybindings for help view
- readd o/O to add positioned tasks
- wrap lines in tasks
- add header
- remove inbox
- move all relevant markoff files/folder into <root>
- increase slug max length
- use ordering file instead of naming convention
- add experimental daily task
- add hints to normal view
- improve help view
- add creation of sections
- allow renaming of sections
- allow deleting of empty sections
- move create action to `n`
- add basic filter
- add jump keymaps
- use prefix number of 01-99
- enforce 10s in namings
- use D/X to delete/archive
- add cursor position to undo action
- add moving of sections/tasks
- normalize on most actions
- put drafts in the path that the result would be in
- open new tasks in insert mode when in vim
- hide preview on empty task
- align styling
- add file watcher
- add config file
- add help menue
- allow creation of tasks
- use EDITOR to update tasks
- wire archiving
- add archiving
- allow renumbering
- link actions to keymaps
- allow edits
- add operations
- add markdown preview
- use tasks to display data
- add loading of tasks
- add tasks
- handle todo namings
- setup basic tui
- init rust project
- add initial readme

### Fixed

- replace rename popup with status message
- allow esc to close help window
- stop `}` jump at section level
- mention archived tasks without title
- scroll only when cursor would go offscreen
- allow test for macos
- add help padding
- align rename message
- show error on creation problem due to external changes
- do not bold selected line
