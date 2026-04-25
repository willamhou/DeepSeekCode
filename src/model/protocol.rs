use crate::tools::types::ToolInput;

#[derive(Debug, Clone)]
pub struct ModelRequest {
    pub system_prompt: String,
    pub task: String,
    pub profile_name: String,
    pub primary_file: Option<String>,
    pub suggested_test_command: Option<String>,
    pub available_tools: Vec<String>,
    pub observations: Vec<Observation>,
}

#[derive(Debug, Clone)]
pub struct Observation {
    pub tool_name: String,
    pub summary: String,
}

#[derive(Debug, Clone)]
pub struct ModelResponse {
    pub message: String,
    pub action: ModelAction,
}

#[derive(Debug, Clone)]
pub enum ModelAction {
    CallTool { tool_name: String, input: ToolInput },
    Finish,
}
