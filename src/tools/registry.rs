use std::env;

use crate::config::types::ApprovalConfig;
use crate::tools::apply_patch::ApplyPatchTool;
use crate::tools::git_diff::GitDiffTool;
use crate::tools::list_files::ListFilesTool;
use crate::tools::read_file::ReadFileTool;
use crate::tools::run_shell::{is_safe_shell_command, RunShellTool};
use crate::tools::search_text::SearchTextTool;
use crate::tools::types::{Tool, ToolInput, ToolOutput};
use crate::error::{app_error, AppResult};
use crate::skills::schema::SkillSpec;

pub struct ToolRegistry {
    tools: Vec<Box<dyn Tool>>,
}

impl ToolRegistry {
    pub fn names(&self) -> Vec<&'static str> {
        self.tools.iter().map(|tool| tool.name()).collect()
    }

    pub fn names_for_policy(&self, policy: &ExecutionPolicy) -> Vec<&'static str> {
        self.tools
            .iter()
            .map(|tool| tool.name())
            .filter(|name| policy.allows_tool(name))
            .collect()
    }

    pub fn execute(&self, name: &str, input: ToolInput) -> AppResult<ToolOutput> {
        let tool = self
            .tools
            .iter()
            .find(|tool| tool.name() == name)
            .ok_or_else(|| app_error(format!("unknown tool: {name}")))?;
        tool.execute(input)
    }

    pub fn execute_with_policy(
        &self,
        name: &str,
        input: ToolInput,
        policy: &ExecutionPolicy,
    ) -> AppResult<ToolOutput> {
        if !policy.allows_tool(name) {
            return Err(app_error(format!("tool blocked by policy: {name}")));
        }

        if name == "apply_patch" && policy.require_write_confirmation && !policy.auto_approve_writes {
            return Err(app_error(
                "write approval required; set DSCODE_AUTO_APPROVE_WRITES=1 or relax the active policy",
            ));
        }

        if name == "run_shell" {
            let command = input
                .get("command")
                .ok_or_else(|| app_error("run_shell requires a command"))?;

            if policy.require_shell_confirmation && !policy.auto_approve_shell {
                return Err(app_error(
                    "shell approval required; set DSCODE_AUTO_APPROVE_SHELL=1 or relax the active policy",
                ));
            }

            if !policy.shell_allowlist.is_empty()
                && !policy
                    .shell_allowlist
                    .iter()
                    .any(|prefix| command.trim().starts_with(prefix))
            {
                return Err(app_error(format!(
                    "shell command blocked by policy allowlist: {command}"
                )));
            }

            if !is_safe_shell_command(command) {
                return Err(app_error(format!("command not allowed: {command}")));
            }
        }

        self.execute(name, input)
    }
}

#[derive(Debug, Clone)]
pub struct ExecutionPolicy {
    allowed_tools: Vec<String>,
    require_write_confirmation: bool,
    require_shell_confirmation: bool,
    shell_allowlist: Vec<String>,
    auto_approve_writes: bool,
    auto_approve_shell: bool,
}

impl ExecutionPolicy {
    pub fn new(approval: &ApprovalConfig, skill: Option<&SkillSpec>) -> Self {
        let (allowed_tools, require_write_confirmation, require_shell_confirmation, shell_allowlist) =
            if let Some(skill) = skill {
                (
                    skill.allowed_tools.clone(),
                    skill.policy.require_write_confirmation,
                    skill.policy.require_shell_confirmation,
                    skill.policy.shell_allowlist.clone(),
                )
            } else {
                (
                    Vec::new(),
                    approval.require_write_confirmation,
                    approval.require_shell_confirmation,
                    Vec::new(),
                )
            };

        Self {
            allowed_tools,
            require_write_confirmation,
            require_shell_confirmation,
            shell_allowlist,
            auto_approve_writes: env_flag("DSCODE_AUTO_APPROVE_WRITES"),
            auto_approve_shell: env_flag("DSCODE_AUTO_APPROVE_SHELL"),
        }
    }

    pub fn allows_tool(&self, name: &str) -> bool {
        self.allowed_tools.is_empty() || self.allowed_tools.iter().any(|tool| tool == name)
    }
}

fn env_flag(name: &str) -> bool {
    matches!(env::var(name).ok().as_deref(), Some("1") | Some("true") | Some("TRUE"))
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
