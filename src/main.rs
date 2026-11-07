//! markoff: a TUI for tasks stored as markdown files.

mod app;
mod archive;
mod cli;
mod config;
mod cursor;
mod edit;
mod editor;
mod filter;
mod heatmap;
mod keymap;
mod load;
mod name;
mod op;
mod repeat;
mod state;
mod task;
mod ui;
mod watch;

use std::process::ExitCode;

use app::App;
use clap::Parser;
use cli::Cli;
use color_eyre::Result;

fn main() -> ExitCode {
    // Wrong args, `--help` and `--version` exit here, before the config is read, so they work
    // with a broken config.
    let cli = Cli::parse();
    // Errors here are expected (no dir, broken config), so one plain line instead of
    // color-eyre's report with location and backtrace hints, that stays for panics.
    match run(cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("markoff: {err:#}");
            ExitCode::FAILURE
        }
    }
}

fn run(cli: Cli) -> Result<()> {
    color_eyre::install()?;

    // Needs no root, an agent may read it before one is set up.
    if matches!(cli.command, Some(cli::Command::Skill)) {
        print!("{}", cli::SKILL);
        return Ok(());
    }
    // Load before entering the TUI, so errors print to a normal terminal.
    let (root, config) = config::from_env(cli.dir)?;
    if let Some(command) = cli.command {
        print!(
            "{}",
            cli::run(command, &root, &config, jiff::Zoned::now().date())?
        );
        return Ok(());
    }
    let app = App::load(root, config)?;

    // `ratatui::run` sets up the terminal, restores it on exit and installs a panic hook.
    ratatui::run(|terminal| app.run(terminal))
}
