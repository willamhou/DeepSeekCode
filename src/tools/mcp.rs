use std::collections::BTreeMap;
use std::sync::{Mutex, OnceLock};

use crate::config::types::AppConfig;
use crate::error::{app_error, AppResult};
use crate::tools::types::{Tool, ToolInput, ToolOutput};
use crate::util::json::{json_value_to_string, JsonValue};

pub const MCP_DYNAMIC_TOOL_PREFIX: &str = "mcp__";
static MCP_DYNAMIC_TOOL_SCHEMAS: OnceLock<Mutex<BTreeMap<String, McpDynamicToolSchema>>> =
    OnceLock::new();

#[derive(Debug, Clone)]
pub struct McpDynamicToolSchema {
    pub description: Option<String>,
    pub input_schema: Option<String>,
}

#[derive(Clone)]
pub struct McpListToolsTool {
    pub config: AppConfig,
}

impl Tool for McpListToolsTool {
    fn name(&self) -> &str {
        "mcp_list_tools"
    }

    fn execute(&self, input: ToolInput) -> AppResult<ToolOutput> {
        let server = input.get("server").filter(|value| !value.trim().is_empty());
        let summary = crate::cli::commands::mcp::list_remote_tools_summary(&self.config, server)?;
        Ok(ToolOutput { summary })
    }
}

#[derive(Clone)]
pub struct McpCallTool {
    pub config: AppConfig,
}

impl Tool for McpCallTool {
    fn name(&self) -> &str {
        "mcp_call"
    }

    fn execute(&self, input: ToolInput) -> AppResult<ToolOutput> {
        let server = input
            .get("server")
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| app_error("mcp_call requires `server`"))?;
        let tool = input
            .get("tool")
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| app_error("mcp_call requires `tool`"))?;
        let arguments = input
            .get("arguments")
            .filter(|value| !value.trim().is_empty());
        let summary = crate::cli::commands::mcp::call_remote_tool_summary(
            &self.config,
            server,
            tool,
            arguments,
        )?;
        Ok(ToolOutput { summary })
    }
}

#[derive(Clone)]
pub struct McpRemoteToolTool {
    pub name: String,
    pub server: String,
    pub tool: String,
    pub config: AppConfig,
}

impl Tool for McpRemoteToolTool {
    fn name(&self) -> &str {
        &self.name
    }

    fn mcp_target(&self) -> Option<(&str, &str)> {
        Some((&self.server, &self.tool))
    }

    fn execute(&self, input: ToolInput) -> AppResult<ToolOutput> {
        let direct_arguments;
        let arguments = if let Some(arguments) = input
            .get("arguments")
            .filter(|value| !value.trim().is_empty())
        {
            Some(arguments)
        } else {
            direct_arguments = tool_input_as_json_object(&input);
            Some(direct_arguments.as_str())
        };
        let summary = crate::cli::commands::mcp::call_remote_tool_summary(
            &self.config,
            &self.server,
            &self.tool,
            arguments,
        )?;
        Ok(ToolOutput { summary })
    }
}

pub fn cache_dynamic_tool_schema(
    registry_name: &str,
    description: Option<String>,
    input_schema: Option<String>,
) {
    let schemas = MCP_DYNAMIC_TOOL_SCHEMAS.get_or_init(|| Mutex::new(BTreeMap::new()));
    if let Ok(mut schemas) = schemas.lock() {
        schemas.insert(
            registry_name.to_string(),
            McpDynamicToolSchema {
                description,
                input_schema,
            },
        );
    }
}

pub fn dynamic_tool_schema(registry_name: &str) -> Option<McpDynamicToolSchema> {
    MCP_DYNAMIC_TOOL_SCHEMAS
        .get()
        .and_then(|schemas| schemas.lock().ok()?.get(registry_name).cloned())
}

pub fn remote_tool_registry_name(server: &str, tool: &str) -> String {
    format!(
        "{MCP_DYNAMIC_TOOL_PREFIX}{}__{}",
        sanitize_tool_name_segment(server),
        sanitize_tool_name_segment(tool)
    )
}

fn tool_input_as_json_object(input: &ToolInput) -> String {
    json_value_to_string(&JsonValue::Object(
        input
            .args
            .iter()
            .filter(|(key, _)| key.as_str() != "arguments")
            .map(|(key, value)| (key.clone(), JsonValue::String(value.clone())))
            .collect(),
    ))
}

fn sanitize_tool_name_segment(value: &str) -> String {
    let mut output = String::new();
    for ch in value.chars().take(28) {
        if ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' {
            output.push(ch);
        } else {
            output.push('_');
        }
    }
    if output.is_empty() {
        "tool".to_string()
    } else {
        output
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root(name: &str) -> std::path::PathBuf {
        let suffix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "deepseek-mcp-tool-{name}-{}-{suffix}",
            std::process::id()
        ))
    }

    fn fake_server_config(root: &std::path::Path) -> AppConfig {
        std::fs::create_dir_all(root).unwrap();
        let server = root.join("server.sh");
        std::fs::write(
            &server,
            r#"#!/bin/sh
while IFS= read -r line; do
  case "$line" in
    *'"method":"initialize"'*)
      printf '%s\n' '{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2025-11-25","capabilities":{"tools":{}},"serverInfo":{"name":"fake","version":"1"}}}'
      ;;
    *'"method":"tools/list"'*)
      printf '%s\n' '{"jsonrpc":"2.0","id":2,"result":{"tools":[{"name":"echo","description":"Echo input","inputSchema":{"type":"object","properties":{"text":{"type":"string"}}}}]}}'
      exit 0
      ;;
    *'"method":"tools/call"'*)
      printf '%s\n' '{"jsonrpc":"2.0","id":2,"result":{"content":[{"type":"text","text":"echo: hello"}],"structuredContent":{"ok":true},"isError":false}}'
      exit 0
      ;;
  esac
done
"#,
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut permissions = std::fs::metadata(&server).unwrap().permissions();
            permissions.set_mode(0o755);
            std::fs::set_permissions(&server, permissions).unwrap();
        }

        let mcp_file = root.join("mcp.json");
        std::fs::write(
            &mcp_file,
            format!(
                r#"{{"mcpServers":{{"fake":{{"transport":"stdio","command":"/bin/sh","args":["{}"]}}}}}}"#,
                server.display()
            ),
        )
        .unwrap();

        let mut config = AppConfig::default();
        config.mcp.project_file = mcp_file.display().to_string();
        config.mcp.user_file = root.join("missing-user.json").display().to_string();
        config
    }

    #[test]
    fn mcp_list_tools_tool_executes_stdio_tools_list() {
        let root = temp_root("list");
        let config = fake_server_config(&root);
        let output = McpListToolsTool { config }
            .execute(ToolInput::new().with_arg("server", "fake"))
            .unwrap();

        assert!(output.summary.contains("fake [stdio]: 1 tool(s)"));
        assert!(output.summary.contains("echo"));

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn mcp_call_tool_executes_stdio_tools_call() {
        let root = temp_root("call");
        let config = fake_server_config(&root);
        let output = McpCallTool { config }
            .execute(
                ToolInput::new()
                    .with_arg("server", "fake")
                    .with_arg("tool", "echo")
                    .with_arg("arguments", r#"{"text":"hello"}"#),
            )
            .unwrap();

        assert!(output.summary.contains("fake/echo [stdio]: ok"));
        assert!(output.summary.contains("echo: hello"));
        assert!(output.summary.contains(r#"{"ok":true}"#));

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn mcp_remote_tool_executes_configured_tool() {
        let root = temp_root("remote-call");
        let config = fake_server_config(&root);
        let output = McpRemoteToolTool {
            name: remote_tool_registry_name("fake", "echo"),
            server: "fake".to_string(),
            tool: "echo".to_string(),
            config,
        }
        .execute(ToolInput::new().with_arg("arguments", r#"{"text":"hello"}"#))
        .unwrap();

        assert!(output.summary.contains("fake/echo [stdio]: ok"));
        assert!(output.summary.contains("echo: hello"));

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn mcp_remote_tool_serializes_direct_schema_arguments() {
        let input = ToolInput::new()
            .with_arg("path", "README.md")
            .with_arg("limit", "5");

        assert_eq!(
            tool_input_as_json_object(&input),
            r#"{"limit":"5","path":"README.md"}"#
        );
    }

    #[test]
    fn dynamic_tool_schema_round_trips_cached_schema() {
        cache_dynamic_tool_schema(
            "mcp__fake__echo",
            Some("Echo input".to_string()),
            Some(r#"{"type":"object"}"#.to_string()),
        );

        let schema = dynamic_tool_schema("mcp__fake__echo").unwrap();

        assert_eq!(schema.description.as_deref(), Some("Echo input"));
        assert_eq!(schema.input_schema.as_deref(), Some(r#"{"type":"object"}"#));
    }

    #[test]
    fn remote_tool_registry_name_sanitizes_segments() {
        assert_eq!(
            remote_tool_registry_name("github prod", "list/issues"),
            "mcp__github_prod__list_issues"
        );
        assert!(
            remote_tool_registry_name(
                "server-name-with-more-than-twenty-eight-chars",
                "tool-name-with-more-than-twenty-eight-chars",
            )
            .len()
                <= 64
        );
    }
}
