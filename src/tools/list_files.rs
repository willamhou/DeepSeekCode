use crate::tools::types::{Tool, ToolInput, ToolOutput};
use crate::error::AppResult;

pub struct ListFilesTool;

impl Tool for ListFilesTool {
    fn name(&self) -> &'static str {
        "list_files"
    }

    fn execute(&self, _input: ToolInput) -> AppResult<ToolOutput> {
        Ok(ToolOutput {
            summary: "list_files not implemented yet".to_string(),
        })
    }
}
