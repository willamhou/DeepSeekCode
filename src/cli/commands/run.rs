use crate::cli::app::RunArgs;
use crate::config::load::load_or_default;
use crate::core::agent::Agent;
use crate::core::context::TaskContext;
use crate::error::AppResult;

pub fn run(args: RunArgs) -> AppResult<()> {
    let config = load_or_default()?;
    let mut agent = Agent::new(config);
    let context = TaskContext::new(args.task, args.skill);
    agent.run(context)
}
