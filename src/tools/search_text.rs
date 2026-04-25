use crate::tools::types::{Tool, ToolInput, ToolOutput};
use crate::error::AppResult;

pub struct SearchTextTool;

impl Tool for SearchTextTool {
    fn name(&self) -> &'static str {
        "search_text"
    }

    fn execute(&self, _input: ToolInput) -> AppResult<ToolOutput> {
        Ok(ToolOutput {
            summary: "search_text not implemented yet".to_string(),
        })
    }
}
