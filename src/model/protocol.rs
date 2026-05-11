use crate::tools::types::ToolInput;

#[derive(Debug, Clone)]
pub struct ModelRequest {
    pub system_prompt: String,
    pub task: String,
    pub image_inputs: Vec<ImageInput>,
    pub profile_name: String,
    pub profile_hints: Vec<String>,
    pub primary_file: Option<String>,
    pub suggested_test_command: Option<String>,
    pub available_tools: Vec<String>,
    pub observations: Vec<Observation>,
    pub todos: Vec<crate::core::todos::Todo>,
    pub planning_mode: bool,
    /// Most recent assistant messages from prior agent loop steps (for `dscode run`
    /// continuity — REPL flows already replay the transcript). Kept compact: caller
    /// pushes the last N (typically 3) to avoid prompt bloat.
    pub recent_steps: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ImageInput {
    pub path: String,
    pub media_type: String,
    pub data_base64: String,
}

#[derive(Debug, Clone)]
pub struct Observation {
    pub tool_name: String,
    pub summary: String,
    pub status: ObservationStatus,
    pub kind: ObservationKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObservationStatus {
    Ok,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ObservationKind {
    FileExcerpt,
    Listing,
    SearchResults,
    Patch,
    Diff,
    ShellOutput,
    Other,
    Todos,
}

impl ObservationKind {
    pub fn from_tool_name(name: &str) -> Self {
        match name {
            "read_file" => Self::FileExcerpt,
            "list_files" => Self::Listing,
            "search_text" => Self::SearchResults,
            "apply_patch" => Self::Patch,
            "git_diff" => Self::Diff,
            "diagnostics" => Self::ShellOutput,
            "run_shell" => Self::ShellOutput,
            "todo_write" => Self::Todos,
            _ => Self::Other,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::FileExcerpt => "file_excerpt",
            Self::Listing => "listing",
            Self::SearchResults => "search_results",
            Self::Patch => "patch",
            Self::Diff => "diff",
            Self::ShellOutput => "shell_output",
            Self::Other => "other",
            Self::Todos => "todos",
        }
    }
}

impl Observation {
    pub fn ok(tool_name: impl Into<String>, summary: impl Into<String>) -> Self {
        let tool_name = tool_name.into();
        let kind = ObservationKind::from_tool_name(&tool_name);
        Self {
            tool_name,
            summary: summary.into(),
            status: ObservationStatus::Ok,
            kind,
        }
    }

    pub fn failed(tool_name: impl Into<String>, summary: impl Into<String>) -> Self {
        let tool_name = tool_name.into();
        let kind = ObservationKind::from_tool_name(&tool_name);
        Self {
            tool_name,
            summary: summary.into(),
            status: ObservationStatus::Failed,
            kind,
        }
    }

    pub fn is_failure(&self) -> bool {
        matches!(self.status, ObservationStatus::Failed)
    }
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

#[derive(Debug, Clone, Default)]
pub struct TokenUsage {
    pub model: Option<String>,
    pub prompt: u64,
    pub completion: u64,
    pub prompt_cache_hit: u64,
    pub prompt_cache_miss: u64,
}

impl TokenUsage {
    pub fn new(prompt: u64, completion: u64) -> Self {
        Self {
            model: None,
            prompt,
            completion,
            prompt_cache_hit: 0,
            prompt_cache_miss: prompt,
        }
    }

    pub fn with_prompt_cache(
        prompt: u64,
        completion: u64,
        prompt_cache_hit: u64,
        prompt_cache_miss: u64,
    ) -> Self {
        let known_prompt = prompt_cache_hit.saturating_add(prompt_cache_miss);
        let prompt_cache_miss = if known_prompt == 0 {
            prompt
        } else {
            prompt_cache_miss
        };
        Self {
            model: None,
            prompt,
            completion,
            prompt_cache_hit,
            prompt_cache_miss,
        }
    }

    pub fn add_assign(&mut self, other: &Self) {
        let self_tokens = self.prompt.saturating_add(self.completion);
        let other_tokens = other.prompt.saturating_add(other.completion);
        if other_tokens > 0 {
            if self_tokens == 0 {
                self.model = other.model.clone();
            } else if self.model != other.model {
                self.model = Some("mixed".to_string());
            }
        }
        self.prompt = self.prompt.saturating_add(other.prompt);
        self.completion = self.completion.saturating_add(other.completion);
        self.prompt_cache_hit = self.prompt_cache_hit.saturating_add(other.prompt_cache_hit);
        self.prompt_cache_miss = self
            .prompt_cache_miss
            .saturating_add(other.prompt_cache_miss);
    }
}
