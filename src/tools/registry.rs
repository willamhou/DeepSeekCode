use crate::tools::apply_patch::ApplyPatchTool;
use crate::tools::git_diff::GitDiffTool;
use crate::tools::list_files::ListFilesTool;
use crate::tools::read_file::ReadFileTool;
use crate::tools::run_shell::RunShellTool;
use crate::tools::search_text::SearchTextTool;
use crate::tools::types::Tool;

pub struct ToolRegistry {
    tools: Vec<Box<dyn Tool>>,
}

impl ToolRegistry {
    pub fn names(&self) -> Vec<&'static str> {
        self.tools.iter().map(|tool| tool.name()).collect()
    }
}

pub fn default_registry() -> ToolRegistry {
    ToolRegistry {
        tools: vec![
            Box::new(ListFilesTool),
            Box::new(ReadFileTool),
            Box::new(SearchTextTool),
            Box::new(ApplyPatchTool),
            Box::new(RunShellTool),
            Box::new(GitDiffTool),
        ],
    }
}

