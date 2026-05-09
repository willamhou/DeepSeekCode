use crate::cli::app::{BenchmarkArgs, RunArgs};
use crate::config::load::load_or_default;
use crate::core::context::TaskContext;
use crate::core::loop_runtime::{AgentLoop, AgentLoopOptions};
use crate::error::AppResult;

pub fn run(args: RunArgs) -> AppResult<()> {
    let config = load_or_default()?;
    let context = TaskContext::new(args.task, args.skill);
    let agent = AgentLoop::new(config.clone());
    let run_result = match args.budget {
        Some(steps) => agent
            .run_with(
                context,
                AgentLoopOptions {
                    steps,
                    ..AgentLoopOptions::default()
                },
            )
            .map(|_| ()),
        None => agent.run(context),
    };
    if args.benchmark_gate {
        println!("post-task benchmark gate: running default benchmark baseline");
        crate::cli::commands::benchmark::run_with_config(config, BenchmarkArgs::default())?;
    }
    run_result
}
