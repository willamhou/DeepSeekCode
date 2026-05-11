use std::collections::BTreeMap;

use crate::error::AppResult;
use crate::util::cancel::CancellationCheck;

#[derive(Debug, Clone, Default)]
pub struct ToolInput {
    pub args: BTreeMap<String, String>,
}

impl ToolInput {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_arg(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.args.insert(key.into(), value.into());
        self
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.args.get(key).map(String::as_str)
    }
}

#[derive(Debug, Clone)]
pub struct ToolOutput {
    pub summary: String,
}

pub trait Tool {
    fn name(&self) -> &str;
    fn mcp_target(&self) -> Option<(&str, &str)> {
        None
    }
    fn execute(&self, input: ToolInput) -> AppResult<ToolOutput>;
    fn execute_with_cancel(
        &self,
        input: ToolInput,
        _cancel_check: Option<&mut dyn CancellationCheck>,
    ) -> AppResult<ToolOutput> {
        self.execute(input)
    }
}
