//! markoff: a TUI for todos stored as markdown files.

mod app;
mod archive;
mod cli;
mod config;
mod daily;
mod edit;
mod editor;
mod filter;
mod history;
mod keymap;
mod load;
mod name;
mod op;
mod order;
mod task;
mod ui;
mod watch;

use std::process::ExitCode;

use app::App;
use color_eyre::Result;

fn main() -> ExitCode {
    if let Some(text) = config::info(std::env::args().skip(1)) {
        print!("{text}");
        return ExitCode::SUCCESS;
    }
    // Errors here are expected (no dir, broken config), so one plain line instead of
    // color-eyre's report with location and backtrace hints, that stays for panics.
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("markoff: {err:#}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<()> {
    color_eyre::install()?;

    // Load before entering the TUI, so errors print to a normal terminal.
    let (root, config, command) = config::from_env()?;
    if let Some(command) = command {
        print!("{}", cli::run(command, &root)?);
        return Ok(());
    }
    let daily = config.daily.map(|name| root.join(name));
    let app = App::load(root, daily)?;

    // `ratatui::run` sets up the terminal, restores it on exit and installs a panic hook.
    ratatui::run(|terminal| app.run(terminal))
}
