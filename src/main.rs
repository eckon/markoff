mod app;
mod config;
mod keymap;
mod load;
mod name;
mod op;
mod task;
mod ui;

use app::App;
use color_eyre::Result;

fn main() -> Result<()> {
    color_eyre::install()?;

    // Load before entering the TUI, so errors print to a normal terminal.
    let root = config::root_dir_from_env()?;
    let sections = load::load(&root.join("todo"))?;

    // `ratatui::run` sets up the terminal, restores it on exit and installs a panic hook.
    ratatui::run(|terminal| App::new(sections).run(terminal))
}
