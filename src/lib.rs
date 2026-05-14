pub mod cli;
pub mod config;
pub mod core;
pub mod error;
pub mod integrations;
pub mod language;
pub mod model;
pub mod repl;
pub mod skills;
pub mod tools;
pub mod tui;
pub mod ui;
pub mod util;
pub mod workspace_trust;

pub use error::AppResult;

pub fn run_main() -> AppResult<()> {
    let cli = match cli::app::Cli::parse() {
        Ok(cli) => cli,
        Err(message) => {
            eprintln!("error: {message}");
            std::process::exit(2);
        }
    };
    cli::run(cli)
}
