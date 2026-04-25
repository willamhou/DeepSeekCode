use crate::cli::app::DiffArgs;
use crate::error::AppResult;
use crate::tools::git_diff::GitDiffTool;
use crate::tools::types::{Tool, ToolInput};

pub fn run(_args: DiffArgs) -> AppResult<()> {
    let output = GitDiffTool.execute(ToolInput::default())?;
    println!("{}", output.summary);
    Ok(())
}
