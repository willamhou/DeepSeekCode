mod cli;
mod config;
mod core;
mod error;
mod language;
mod model;
mod skills;
mod tools;
mod ui;

use error::AppResult;

fn main() -> AppResult<()> {
    let cli = cli::app::Cli::parse();
    cli::run(cli)
}
