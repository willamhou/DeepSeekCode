use std::env;
use std::process::Command;

use crate::config::types::ModelConfig;
use crate::error::AppResult;
use crate::error::app_error;
use crate::model::client::ModelClient;
use crate::model::protocol::{ModelAction, ModelRequest, ModelResponse};
use crate::tools::types::ToolInput;

pub struct DeepSeekClient {
    pub config: ModelConfig,
}

impl ModelClient for DeepSeekClient {
    fn respond(&self, input: ModelRequest) -> AppResult<ModelResponse> {
        if let Ok(api_key) = env::var(&self.config.api_key_env) {
            if !api_key.trim().is_empty() {
                if let Ok(response) = self.respond_remote(&input, &api_key) {
                    return Ok(response);
                }
            }
        }

        Ok(self.respond_offline(input))
    }
}

impl DeepSeekClient {
    fn respond_remote(&self, input: &ModelRequest, api_key: &str) -> AppResult<ModelResponse> {
        let endpoint = format!("{}/chat/completions", self.config.base_url.trim_end_matches('/'));
        let system_prompt = build_remote_system_prompt(&input.system_prompt);
        let user_prompt = build_user_prompt(input);
        let body = format!(
            "{{\"model\":\"{}\",\"temperature\":0,\"messages\":[{{\"role\":\"system\",\"content\":\"{}\"}},{{\"role\":\"user\",\"content\":\"{}\"}}]}}",
            json_escape(&self.config.model),
            json_escape(&system_prompt),
            json_escape(&user_prompt)
        );

        let output = Command::new("curl")
            .args([
                "-sS",
                "-X",
                "POST",
                &endpoint,
                "-H",
                &format!("Authorization: Bearer {api_key}"),
                "-H",
                "Content-Type: application/json",
                "--data-binary",
                &body,
            ])
            .output()?;

        if !output.status.success() {
            return Err(app_error(format!(
                "deepseek request failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            )));
        }

        let body = String::from_utf8_lossy(&output.stdout);
        let content = extract_message_content(&body)?;
        parse_planner_response(&content)
    }

    fn respond_offline(&self, input: ModelRequest) -> ModelResponse {
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
            return ModelResponse {
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
            };
        }

        if edit_request.is_none() {
            if let Some(query) = search_query {
                if !used_tools.contains(&"search_text") && input.available_tools.iter().any(|name| name == "search_text") {
                    return ModelResponse {
                        message: format!("{} planner is searching for `{query}`.", self.config.model),
                        action: ModelAction::CallTool {
                            tool_name: "search_text".to_string(),
                            input: ToolInput::new()
                                .with_arg("root", ".")
                                .with_arg("query", query)
                                .with_arg("limit", "20"),
                        },
                    };
                }
            }
        }

        if let Some(edit_request) = edit_request.as_ref() {
            if !used_tools.contains(&"read_file") && input.available_tools.iter().any(|name| name == "read_file") {
                return ModelResponse {
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
                };
            }
        }

        if let Some(edit_request) = edit_request.as_ref() {
            if !used_tools.contains(&"apply_patch")
                && input.available_tools.iter().any(|name| name == "apply_patch")
            {
                return ModelResponse {
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
                };
            }
        }

        if edit_request.is_none() {
            if let Some(primary_file) = input.primary_file.as_deref() {
                if !used_tools.contains(&"read_file") && input.available_tools.iter().any(|name| name == "read_file") {
                    return ModelResponse {
                        message: format!("{} planner is reading the primary file.", self.config.model),
                        action: ModelAction::CallTool {
                            tool_name: "read_file".to_string(),
                            input: ToolInput::new()
                                .with_arg("path", primary_file)
                                .with_arg("max_lines", "40"),
                        },
                    };
                }
            }
        }

        if used_tools.contains(&"apply_patch")
            && !used_tools.contains(&"git_diff")
            && input.available_tools.iter().any(|name| name == "git_diff")
        {
            return ModelResponse {
                message: format!("{} planner is reviewing the resulting diff.", self.config.model),
                action: ModelAction::CallTool {
                    tool_name: "git_diff".to_string(),
                    input: ToolInput::new(),
                },
            };
        }

        if let Some(test_command) = input.suggested_test_command.as_deref() {
            if wants_validation(&task_lower)
                && !used_tools.contains(&"run_shell")
                && input.available_tools.iter().any(|name| name == "run_shell")
            {
                return ModelResponse {
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
                };
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

        ModelResponse {
            message,
            action: ModelAction::Finish,
        }
    }
}

fn build_remote_system_prompt(base: &str) -> String {
    format!(
        "{base}\nReturn exactly one action in this format:\nACTION: FINISH\nMESSAGE: short summary\nor\nACTION: TOOL <tool_name>\nMESSAGE: short reason\nARG key=value\nARG key=value\nOnly use listed tools. Keep arguments plain text."
    )
}

fn build_user_prompt(input: &ModelRequest) -> String {
    let mut prompt = String::new();
    prompt.push_str(&format!("Task: {}\n", input.task));
    prompt.push_str(&format!("Profile: {}\n", input.profile_name));
    if let Some(primary_file) = input.primary_file.as_deref() {
        prompt.push_str(&format!("Primary file: {primary_file}\n"));
    }
    if let Some(command) = input.suggested_test_command.as_deref() {
        prompt.push_str(&format!("Suggested test command: {command}\n"));
    }
    prompt.push_str(&format!("Available tools: {}\n", input.available_tools.join(", ")));
    prompt.push_str("Observations:\n");
    if input.observations.is_empty() {
        prompt.push_str("- none\n");
    } else {
        for observation in &input.observations {
            let summary = observation
                .summary
                .lines()
                .take(6)
                .collect::<Vec<_>>()
                .join(" | ");
            prompt.push_str(&format!(
                "- tool={} summary={}\n",
                observation.tool_name, summary
            ));
        }
    }
    prompt
}

fn parse_planner_response(content: &str) -> AppResult<ModelResponse> {
    let mut action = None::<ModelAction>;
    let mut message = String::new();
    let mut input = ToolInput::new();

    for raw_line in content.lines() {
        let line = raw_line.trim();
        if let Some(rest) = line.strip_prefix("ACTION:") {
            let rest = rest.trim();
            if rest == "FINISH" {
                action = Some(ModelAction::Finish);
            } else if let Some(tool_name) = rest.strip_prefix("TOOL ") {
                action = Some(ModelAction::CallTool {
                    tool_name: tool_name.trim().to_string(),
                    input: ToolInput::new(),
                });
            }
        } else if let Some(rest) = line.strip_prefix("MESSAGE:") {
            message = rest.trim().to_string();
        } else if let Some(rest) = line.strip_prefix("ARG ") {
            if let Some((key, value)) = rest.split_once('=') {
                input = input.with_arg(key.trim(), value.trim());
            }
        }
    }

    let action = match action {
        Some(ModelAction::CallTool { tool_name, .. }) => ModelAction::CallTool { tool_name, input },
        Some(ModelAction::Finish) => ModelAction::Finish,
        None => return Err(app_error("model response did not contain a valid ACTION line")),
    };

    if message.is_empty() {
        message = "DeepSeek responded without a planner message.".to_string();
    }

    Ok(ModelResponse { message, action })
}

fn extract_message_content(body: &str) -> AppResult<String> {
    let marker = "\"content\":\"";
    let start = body
        .find(marker)
        .ok_or_else(|| app_error("deepseek response missing message content"))?
        + marker.len();

    let bytes = body.as_bytes();
    let mut index = start;
    let mut escaped = false;
    let mut output = String::new();

    while index < bytes.len() {
        let byte = bytes[index];
        if escaped {
            match byte {
                b'n' => output.push('\n'),
                b'r' => output.push('\r'),
                b't' => output.push('\t'),
                b'\\' => output.push('\\'),
                b'"' => output.push('"'),
                _ => output.push(byte as char),
            }
            escaped = false;
        } else if byte == b'\\' {
            escaped = true;
        } else if byte == b'"' {
            return Ok(output);
        } else {
            output.push(byte as char);
        }
        index += 1;
    }

    Err(app_error("unterminated content string in deepseek response"))
}

fn json_escape(value: &str) -> String {
    let mut output = String::new();
    for ch in value.chars() {
        match ch {
            '"' => output.push_str("\\\""),
            '\\' => output.push_str("\\\\"),
            '\n' => output.push_str("\\n"),
            '\r' => output.push_str("\\r"),
            '\t' => output.push_str("\\t"),
            _ => output.push(ch),
        }
    }
    output
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
    quoted_segments(task).into_iter().next()
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

#[cfg(test)]
mod tests {
    use super::{extract_message_content, parse_planner_response};
    use crate::model::protocol::ModelAction;

    #[test]
    fn parses_planner_tool_response() {
        let response = parse_planner_response(
            "ACTION: TOOL read_file\nMESSAGE: inspect file\nARG path=README.md\nARG max_lines=20",
        )
        .unwrap();

        match response.action {
            ModelAction::CallTool { tool_name, input } => {
                assert_eq!(tool_name, "read_file");
                assert_eq!(input.get("path"), Some("README.md"));
                assert_eq!(input.get("max_lines"), Some("20"));
            }
            ModelAction::Finish => panic!("expected tool call"),
        }
    }

    #[test]
    fn extracts_json_content_string() {
        let body = r#"{"choices":[{"message":{"role":"assistant","content":"ACTION: FINISH\nMESSAGE: done"}}]}"#;
        let content = extract_message_content(body).unwrap();
        assert!(content.contains("ACTION: FINISH"));
    }
}
