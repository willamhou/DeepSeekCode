use crate::tools::types::{Tool, ToolInput, ToolOutput};
use crate::error::AppResult;

pub struct ReadFileTool;

impl Tool for ReadFileTool {
    fn name(&self) -> &'static str {
        "read_file"
    }

    fn execute(&self, _input: ToolInput) -> AppResult<ToolOutput> {
        Ok(ToolOutput {
            summary: "read_file not implemented yet".to_string(),
        })
    }
}
