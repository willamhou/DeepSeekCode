pub mod app;
pub mod commands;

use crate::error::AppResult;

pub fn run(cli: app::Cli) -> AppResult<()> {
    match cli.command.unwrap_or_default() {
        app::Command::Benchmark(args) => commands::benchmark::run(args),
        app::Command::Dogfood(action) => commands::dogfood::run(action),
        app::Command::Chat(args) => commands::chat::run(args),
        app::Command::Completion(shell) => commands::completion::run(shell),
        app::Command::Run(args) => commands::run::run(args),
        app::Command::Diff(args) => commands::diff::run(args),
        app::Command::Resume(args) => commands::resume::run(args),
        app::Command::Config(args) => commands::config::run(args),
        app::Command::Doctor(args) => commands::doctor::run(args),
        app::Command::Smoke(args) => commands::smoke::run(args),
        app::Command::Pr(action) => commands::pr::run(action),
        app::Command::Version => commands::version::run(),
    }
}
