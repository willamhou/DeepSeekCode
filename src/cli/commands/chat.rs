use crate::cli::app::ChatArgs;
use crate::config::load::load_or_default;
use crate::error::AppResult;
use crate::repl::Repl;

pub fn run(args: ChatArgs) -> AppResult<()> {
    let config = load_or_default()?;
    let mut repl = Repl::new(config, args.skill);
    repl.run()
}
