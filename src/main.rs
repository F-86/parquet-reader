mod app;
mod cli;
mod data;
mod error;
mod file_browser;
mod formatting;
mod tui;

use clap::Parser;

use crate::{cli::CliArgs, error::Result};

fn main() {
    if let Err(error) = run() {
        eprintln!("error: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let args = CliArgs::parse();
    let config = args.into_config()?;
    tui::run(config)
}
