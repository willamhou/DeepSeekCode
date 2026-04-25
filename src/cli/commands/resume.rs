use crate::cli::app::ResumeArgs;
use crate::config::load::load_or_default;
use crate::core::session::SessionStore;
use crate::error::AppResult;

pub fn run(args: ResumeArgs) -> AppResult<()> {
    let config = load_or_default()?;
    let store = SessionStore::new(config.workspace.session_dir());
    let snapshot = store.load_latest(args.session.as_deref())?;
    println!("Resuming session: {}", snapshot.id);
    println!("Task: {}", snapshot.task);
    Ok(())
}
