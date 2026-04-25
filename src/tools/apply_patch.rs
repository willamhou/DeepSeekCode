use crate::tools::types::{Tool, ToolInput, ToolOutput};
use crate::error::AppResult;

pub struct ApplyPatchTool;

impl Tool for ApplyPatchTool {
    fn name(&self) -> &'static str {
        "apply_patch"
    }

    fn execute(&self, _input: ToolInput) -> AppResult<ToolOutput> {
        Ok(ToolOutput {
            summary: "apply_patch not implemented yet".to_string(),
        })
    }
}
