mod app;
mod keymap;
mod load;
mod name;
mod task;
mod ui;

use app::App;
use color_eyre::Result;

fn main() -> Result<()> {
    color_eyre::install()?;
    // `ratatui::run` sets up the terminal, restores it on exit and installs a panic hook.
    ratatui::run(|terminal| App::new().run(terminal))
}
