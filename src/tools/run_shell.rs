use crate::tools::types::{Tool, ToolInput, ToolOutput};
use crate::error::AppResult;

pub struct RunShellTool;

impl Tool for RunShellTool {
    fn name(&self) -> &'static str {
        "run_shell"
    }

    fn execute(&self, _input: ToolInput) -> AppResult<ToolOutput> {
        Ok(ToolOutput {
            summary: "run_shell not implemented yet".to_string(),
        })
    }
}
