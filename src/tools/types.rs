use std::collections::BTreeMap;

use crate::error::AppResult;
#[derive(Debug, Clone, Default)]
pub struct ToolInput {
    pub args: BTreeMap<String, String>,
}

#[derive(Debug, Clone)]
pub struct ToolOutput {
    pub summary: String,
}

pub trait Tool {
    fn name(&self) -> &'static str;
    fn execute(&self, input: ToolInput) -> AppResult<ToolOutput>;
}
