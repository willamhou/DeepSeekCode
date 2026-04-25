use crate::config::types::ModelConfig;
use crate::error::AppResult;
use crate::model::client::ModelClient;
use crate::model::protocol::{ModelAction, ModelRequest, ModelResponse};
use crate::tools::types::ToolInput;

pub struct DeepSeekClient {
    pub config: ModelConfig,
}

impl ModelClient for DeepSeekClient {
    fn respond(&self, input: ModelRequest) -> AppResult<ModelResponse> {
        let task = input.task.clone();
        let task_lower = task.to_lowercase();
        let used_tools = input
            .observations
            .iter()
            .map(|observation| observation.tool_name.as_str())
            .collect::<Vec<_>>();
        let search_query = derive_search_query(&task);
        let edit_request = derive_edit_request(&task);

        if !used_tools.contains(&"list_files") && input.available_tools.iter().any(|name| name == "list_files") {
            return Ok(ModelResponse {
                message: format!(
                    "{} planner is exploring the repository layout first.",
                    self.config.model
                ),
                action: ModelAction::CallTool {
                    tool_name: "list_files".to_string(),
                    input: ToolInput::new()
                        .with_arg("root", ".")
                        .with_arg("max_depth", "2")
                        .with_arg("limit", "20"),
                },
            });
        }

        if edit_request.is_none() {
            if let Some(query) = search_query {
                if !used_tools.contains(&"search_text") && input.available_tools.iter().any(|name| name == "search_text") {
                    return Ok(ModelResponse {
                        message: format!("{} planner is searching for `{query}`.", self.config.model),
                        action: ModelAction::CallTool {
                            tool_name: "search_text".to_string(),
                            input: ToolInput::new()
                                .with_arg("root", ".")
                                .with_arg("query", query)
                                .with_arg("limit", "20"),
                        },
                    });
                }
            }
        }

        if let Some(edit_request) = edit_request.as_ref() {
            if !used_tools.contains(&"read_file") && input.available_tools.iter().any(|name| name == "read_file") {
                return Ok(ModelResponse {
                    message: format!(
                        "{} planner is reading the edit target before applying changes.",
                        self.config.model
                    ),
                    action: ModelAction::CallTool {
                        tool_name: "read_file".to_string(),
                        input: ToolInput::new()
                            .with_arg("path", edit_request.path.clone())
                            .with_arg("max_lines", "40"),
                    },
                });
            }
        }

        if let Some(edit_request) = edit_request.as_ref() {
            if !used_tools.contains(&"apply_patch")
                && input.available_tools.iter().any(|name| name == "apply_patch")
            {
                return Ok(ModelResponse {
                    message: format!(
                        "{} planner is applying a direct text replacement in {}.",
                        self.config.model, edit_request.path
                    ),
                    action: ModelAction::CallTool {
                        tool_name: "apply_patch".to_string(),
                        input: ToolInput::new()
                            .with_arg("path", edit_request.path.clone())
                            .with_arg("find", edit_request.find.clone())
                            .with_arg("replace", edit_request.replace.clone()),
                    },
                });
            }
        }

        if edit_request.is_none() {
            if let Some(primary_file) = input.primary_file.as_deref() {
                if !used_tools.contains(&"read_file") && input.available_tools.iter().any(|name| name == "read_file") {
                    return Ok(ModelResponse {
                        message: format!("{} planner is reading the primary file.", self.config.model),
                        action: ModelAction::CallTool {
                            tool_name: "read_file".to_string(),
                            input: ToolInput::new()
                                .with_arg("path", primary_file)
                                .with_arg("max_lines", "40"),
                        },
                    });
                }
            }
        }

        if used_tools.contains(&"apply_patch")
            && !used_tools.contains(&"git_diff")
            && input.available_tools.iter().any(|name| name == "git_diff")
        {
            return Ok(ModelResponse {
                message: format!("{} planner is reviewing the resulting diff.", self.config.model),
                action: ModelAction::CallTool {
                    tool_name: "git_diff".to_string(),
                    input: ToolInput::new(),
                },
            });
        }

        if let Some(test_command) = input.suggested_test_command.as_deref() {
            if wants_validation(&task_lower)
                && !used_tools.contains(&"run_shell")
                && input.available_tools.iter().any(|name| name == "run_shell")
            {
                return Ok(ModelResponse {
                    message: format!(
                        "{} planner is validating with `{}`.",
                        self.config.model, test_command
                    ),
                    action: ModelAction::CallTool {
                        tool_name: "run_shell".to_string(),
                        input: ToolInput::new()
                            .with_arg("cwd", ".")
                            .with_arg("command", test_command),
                    },
                });
            }
        }

        let mut message = format!(
            "{} offline planner finished after {} observation(s) for {}.",
            self.config.model,
            input.observations.len(),
            input.profile_name
        );

        if !input.system_prompt.is_empty() {
            let prompt_preview = input
                .system_prompt
                .lines()
                .next()
                .unwrap_or("")
                .trim();
            if !prompt_preview.is_empty() {
                message.push_str(&format!(" Prompt frame: {prompt_preview}"));
            }
        }

        if let Some(last) = input.observations.last() {
            message.push_str(&format!(" Last observation came from {}.", last.tool_name));
        }

        Ok(ModelResponse {
            message,
            action: ModelAction::Finish,
        })
    }
}

fn derive_search_query(task: &str) -> Option<String> {
    if let Some(quoted) = first_quoted_segment(task) {
        return Some(quoted);
    }

    for marker in ["search ", "find ", "grep ", "look for "] {
        if let Some(index) = task.find(marker) {
            let value = task[index + marker.len()..]
                .split_whitespace()
                .take(3)
                .collect::<Vec<_>>()
                .join(" ");
            if !value.is_empty() {
                return Some(value);
            }
        }
    }

    None
}

fn first_quoted_segment(task: &str) -> Option<String> {
    let bytes = task.as_bytes();
    let mut start = None;
    for (index, byte) in bytes.iter().enumerate() {
        if *byte == b'"' {
            if let Some(begin) = start {
                let segment = task[begin + 1..index].trim();
                if !segment.is_empty() {
                    return Some(segment.to_string());
                }
                start = None;
            } else {
                start = Some(index);
            }
        }
    }
    None
}

fn wants_validation(task: &str) -> bool {
    ["test", "fix", "validate", "check", "lint"].iter().any(|word| task.contains(word))
}

#[derive(Debug, Clone)]
struct EditRequest {
    path: String,
    find: String,
    replace: String,
}

fn derive_edit_request(task: &str) -> Option<EditRequest> {
    let task_lower = task.to_lowercase();
    if !task_lower.contains("replace ") || !task_lower.contains(" with ") || !task_lower.contains(" in ") {
        return None;
    }

    let quoted = quoted_segments(task);
    if quoted.len() < 2 {
        return None;
    }

    let in_index = task_lower.rfind(" in ")?;
    let path = task[in_index + 4..].trim().trim_matches('`').trim().to_string();
    if path.is_empty() {
        return None;
    }

    Some(EditRequest {
        path,
        find: quoted[0].clone(),
        replace: quoted[1].clone(),
    })
}

fn quoted_segments(task: &str) -> Vec<String> {
    let bytes = task.as_bytes();
    let mut start = None;
    let mut values = Vec::new();

    for (index, byte) in bytes.iter().enumerate() {
        if *byte == b'"' {
            if let Some(begin) = start {
                let segment = task[begin + 1..index].trim();
                if !segment.is_empty() {
                    values.push(segment.to_string());
                }
                start = None;
            } else {
                start = Some(index);
            }
        }
    }

    values
}
