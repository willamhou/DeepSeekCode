use crate::cli::app::ChatArgs;
use crate::config::load::load_or_default;
use crate::core::agent::Agent;
use crate::core::context::TaskContext;
use crate::error::AppResult;

pub fn run(args: ChatArgs) -> AppResult<()> {
    let config = load_or_default()?;
    let mut agent = Agent::new(config);
    let task = args.task.unwrap_or_else(|| "Start interactive session".to_string());
    let context = TaskContext::new(task, args.skill);
    agent.run(context)
}
