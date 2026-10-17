//! markoff: a TUI for todos stored as markdown files.

mod app;
mod archive;
mod config;
mod daily;
mod edit;
mod editor;
mod filter;
mod keymap;
mod load;
mod name;
mod op;
mod order;
mod task;
mod ui;
mod watch;

use app::App;
use color_eyre::Result;

fn main() -> Result<()> {
    color_eyre::install()?;

    // Load before entering the TUI, so errors print to a normal terminal.
    let (root, config) = config::from_env()?;
    let daily = config.daily.map(|name| root.join(name));
    let app = App::load(root, daily)?;

    // `ratatui::run` sets up the terminal, restores it on exit and installs a panic hook.
    ratatui::run(|terminal| app.run(terminal))
}
