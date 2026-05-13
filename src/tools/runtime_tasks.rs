use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::config::types::AppConfig;
use crate::core::runtime::{
    automation_to_json, event_to_json, item_to_json, json_array, json_object, task_to_json,
    thread_to_json, RuntimeStore, TaskRecord,
};
use crate::error::{app_error, AppResult};
use crate::tools::run_shell::RunShellTool;
use crate::tools::types::{Tool, ToolInput, ToolOutput};
use crate::util::cancel::CancellationCheck;
use crate::util::json::{
    json_as_array, json_as_string, json_as_u64, json_value_to_string, parse_json_value,
    parse_root_object, JsonValue,
};

pub struct TaskCreateTool {
    store: RuntimeStore,
}

pub struct TaskListTool {
    store: RuntimeStore,
}

pub struct TaskReadTool {
    store: RuntimeStore,
}

pub struct TaskCancelTool {
    store: RuntimeStore,
}

pub struct TaskGateRunTool;

pub struct PrAttemptRecordTool {
    root: PathBuf,
}

pub struct PrAttemptListTool {
    root: PathBuf,
}

pub struct PrAttemptReadTool {
    root: PathBuf,
}

pub struct PrAttemptPreflightTool {
    root: PathBuf,
}

pub struct AutomationListTool {
    store: RuntimeStore,
}

pub struct AutomationReadTool {
    store: RuntimeStore,
}

pub struct AutomationCreateTool {
    store: RuntimeStore,
}

pub struct AutomationRunTool {
    store: RuntimeStore,
}

pub struct AutomationUpdateTool {
    store: RuntimeStore,
}

pub struct AutomationPauseTool {
    store: RuntimeStore,
}

pub struct AutomationResumeTool {
    store: RuntimeStore,
}

pub struct AutomationDeleteTool {
    store: RuntimeStore,
}

pub struct AgentSpawnTool {
    store: RuntimeStore,
    config: AppConfig,
}

pub struct AgentResultTool {
    store: RuntimeStore,
}

pub struct AgentListTool {
    store: RuntimeStore,
}

pub struct AgentCancelTool {
    store: RuntimeStore,
}

pub struct AgentCloseTool {
    store: RuntimeStore,
}

pub struct AgentResumeTool {
    store: RuntimeStore,
}

pub struct AgentSendInputTool {
    store: RuntimeStore,
}

impl TaskCreateTool {
    pub fn new(config: &AppConfig) -> Self {
        Self {
            store: runtime_store(config),
        }
    }
}

impl TaskListTool {
    pub fn new(config: &AppConfig) -> Self {
        Self {
            store: runtime_store(config),
        }
    }
}

impl TaskReadTool {
    pub fn new(config: &AppConfig) -> Self {
        Self {
            store: runtime_store(config),
        }
    }
}

impl TaskCancelTool {
    pub fn new(config: &AppConfig) -> Self {
        Self {
            store: runtime_store(config),
        }
    }
}

impl PrAttemptRecordTool {
    pub fn new(config: &AppConfig) -> Self {
        Self {
            root: pr_attempt_root(config),
        }
    }
}

impl PrAttemptListTool {
    pub fn new(config: &AppConfig) -> Self {
        Self {
            root: pr_attempt_root(config),
        }
    }
}

impl PrAttemptReadTool {
    pub fn new(config: &AppConfig) -> Self {
        Self {
            root: pr_attempt_root(config),
        }
    }
}

impl PrAttemptPreflightTool {
    pub fn new(config: &AppConfig) -> Self {
        Self {
            root: pr_attempt_root(config),
        }
    }
}

impl AutomationListTool {
    pub fn new(config: &AppConfig) -> Self {
        Self {
            store: runtime_store(config),
        }
    }
}

impl AutomationReadTool {
    pub fn new(config: &AppConfig) -> Self {
        Self {
            store: runtime_store(config),
        }
    }
}

impl AutomationCreateTool {
    pub fn new(config: &AppConfig) -> Self {
        Self {
            store: runtime_store(config),
        }
    }
}

impl AutomationRunTool {
    pub fn new(config: &AppConfig) -> Self {
        Self {
            store: runtime_store(config),
        }
    }
}

impl AutomationUpdateTool {
    pub fn new(config: &AppConfig) -> Self {
        Self {
            store: runtime_store(config),
        }
    }
}

impl AutomationPauseTool {
    pub fn new(config: &AppConfig) -> Self {
        Self {
            store: runtime_store(config),
        }
    }
}

impl AutomationResumeTool {
    pub fn new(config: &AppConfig) -> Self {
        Self {
            store: runtime_store(config),
        }
    }
}

impl AutomationDeleteTool {
    pub fn new(config: &AppConfig) -> Self {
        Self {
            store: runtime_store(config),
        }
    }
}

impl AgentSpawnTool {
    pub fn new(config: &AppConfig) -> Self {
        Self {
            store: runtime_store(config),
            config: config.clone(),
        }
    }
}

impl AgentResultTool {
    pub fn new(config: &AppConfig) -> Self {
        Self {
            store: runtime_store(config),
        }
    }
}

impl AgentListTool {
    pub fn new(config: &AppConfig) -> Self {
        Self {
            store: runtime_store(config),
        }
    }
}

impl AgentCancelTool {
    pub fn new(config: &AppConfig) -> Self {
        Self {
            store: runtime_store(config),
        }
    }
}

impl AgentCloseTool {
    pub fn new(config: &AppConfig) -> Self {
        Self {
            store: runtime_store(config),
        }
    }
}

impl AgentResumeTool {
    pub fn new(config: &AppConfig) -> Self {
        Self {
            store: runtime_store(config),
        }
    }
}

impl AgentSendInputTool {
    pub fn new(config: &AppConfig) -> Self {
        Self {
            store: runtime_store(config),
        }
    }
}

impl Tool for TaskCreateTool {
    fn name(&self) -> &str {
        "task_create"
    }

    fn execute(&self, input: ToolInput) -> AppResult<ToolOutput> {
        let summary = required_nonempty_any(&input, &["prompt", "summary"], "task_create")?;
        let kind = input
            .get("kind")
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("agent")
            .to_string();
        let status = input
            .get("status")
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("pending")
            .to_string();
        let task = self.store.create_task(
            optional_arg(&input, "session_id"),
            optional_arg(&input, "thread_id"),
            optional_arg(&input, "parent_task_id"),
            kind,
            status,
            summary,
        )?;
        Ok(json_output(task_to_json(&task)))
    }
}

impl Tool for TaskListTool {
    fn name(&self) -> &str {
        "task_list"
    }

    fn execute(&self, input: ToolInput) -> AppResult<ToolOutput> {
        let limit = parse_limit(&input, 20, 100);
        let tasks = self.store.list_tasks(
            optional_arg(&input, "session_id"),
            optional_arg(&input, "thread_id"),
            limit,
        )?;
        let items = tasks.iter().map(task_to_json).collect::<Vec<_>>();
        Ok(json_output(json_object([
            (
                "summary",
                JsonValue::String(format!("{} durable task(s)", items.len())),
            ),
            ("tasks", json_array(items)),
        ])))
    }
}

impl Tool for TaskReadTool {
    fn name(&self) -> &str {
        "task_read"
    }

    fn execute(&self, input: ToolInput) -> AppResult<ToolOutput> {
        let task_id = required_nonempty_any(&input, &["task_id", "id"], "task_read")?;
        let task = self.store.load_task(&task_id)?;
        Ok(json_output(task_to_json(&task)))
    }
}

impl Tool for TaskCancelTool {
    fn name(&self) -> &str {
        "task_cancel"
    }

    fn execute(&self, input: ToolInput) -> AppResult<ToolOutput> {
        let task_id = required_nonempty_any(&input, &["task_id", "id"], "task_cancel")?;
        let reason = input
            .get("reason")
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("cancelled by task_cancel")
            .to_string();
        let (task, event) = self.store.cancel_task(&task_id, reason)?;
        let event_json = event.as_ref().map(event_to_json).unwrap_or(JsonValue::Null);
        Ok(json_output(json_object([
            ("task", task_to_json(&task)),
            ("event", event_json),
        ])))
    }
}

impl Tool for TaskGateRunTool {
    fn name(&self) -> &str {
        "task_gate_run"
    }

    fn execute(&self, input: ToolInput) -> AppResult<ToolOutput> {
        self.execute_with_cancel(input, None)
    }

    fn execute_with_cancel(
        &self,
        input: ToolInput,
        cancel_check: Option<&mut dyn CancellationCheck>,
    ) -> AppResult<ToolOutput> {
        let gate = required_nonempty_any(&input, &["gate"], "task_gate_run")?;
        validate_gate(&gate)?;
        let command = required_nonempty_any(&input, &["command"], "task_gate_run")?;
        let mut shell_input = ToolInput::new().with_arg("command", command.clone());
        if let Some(cwd) = optional_arg(&input, "cwd") {
            shell_input = shell_input.with_arg("cwd", cwd);
        }
        let output = RunShellTool.execute_with_cancel(shell_input, cancel_check)?;
        let mut summary = String::new();
        summary.push_str(&format!("meta.gate={gate}\n"));
        summary.push_str(&format!("meta.command={command}\n"));
        if let Some(timeout_ms) = optional_arg(&input, "timeout_ms") {
            summary.push_str(&format!("meta.timeout_ms={timeout_ms}\n"));
        }
        summary.push_str(&output.summary);
        Ok(ToolOutput { summary })
    }
}

impl Tool for PrAttemptRecordTool {
    fn name(&self) -> &str {
        "pr_attempt_record"
    }

    fn execute(&self, input: ToolInput) -> AppResult<ToolOutput> {
        let summary = required_nonempty_any(&input, &["summary"], "pr_attempt_record")?;
        let cwd = optional_arg(&input, "cwd").unwrap_or(".");
        let diff = run_git_capture(cwd, &["diff", "--binary", "--no-color"])?;
        if diff.trim().is_empty() {
            return Err(app_error("No working-tree diff to record as an attempt."));
        }
        fs::create_dir_all(&self.root)?;
        let id = new_attempt_id();
        let patch_path = self.root.join(format!("{id}.patch"));
        fs::write(&patch_path, diff.as_bytes())?;

        let attempt = PrAttemptRecord {
            id,
            task_id: optional_arg(&input, "task_id").map(str::to_string),
            attempt_group_id: optional_arg(&input, "attempt_group_id")
                .map(str::to_string)
                .unwrap_or_else(new_attempt_group_id),
            attempt_index: parse_u32_arg(&input, "attempt_index", 1).max(1),
            attempt_count: parse_u32_arg(&input, "attempt_count", 1).max(1),
            summary,
            verification: parse_verification(optional_arg(&input, "verification"))?,
            changed_files: git_lines(cwd, &["diff", "--name-only"])?,
            base_sha: optional_git_output(cwd, &["rev-parse", "HEAD"]),
            head_sha: optional_git_output(cwd, &["rev-parse", "HEAD"]),
            branch: optional_git_output(cwd, &["rev-parse", "--abbrev-ref", "HEAD"]),
            cwd: cwd.to_string(),
            patch_path: patch_path.display().to_string(),
            recorded_at: epoch_label(),
        };
        self.write_attempt(&attempt)?;
        Ok(json_output(json_object([
            (
                "summary",
                JsonValue::String("PR attempt recorded".to_string()),
            ),
            ("attempt", pr_attempt_to_json(&attempt)),
        ])))
    }
}

impl Tool for PrAttemptListTool {
    fn name(&self) -> &str {
        "pr_attempt_list"
    }

    fn execute(&self, input: ToolInput) -> AppResult<ToolOutput> {
        let task_id = optional_arg(&input, "task_id");
        let limit = parse_limit(&input, 20, 100);
        let mut attempts = self.list_attempts(task_id)?;
        attempts.truncate(limit);
        let items = attempts.iter().map(pr_attempt_to_json).collect::<Vec<_>>();
        Ok(json_output(json_object([
            (
                "summary",
                JsonValue::String(format!("{} PR attempt(s)", items.len())),
            ),
            ("attempts", json_array(items)),
        ])))
    }
}

impl Tool for PrAttemptReadTool {
    fn name(&self) -> &str {
        "pr_attempt_read"
    }

    fn execute(&self, input: ToolInput) -> AppResult<ToolOutput> {
        let attempt_id = required_nonempty_any(&input, &["attempt_id", "id"], "pr_attempt_read")?;
        let attempt = self.load_attempt(&attempt_id)?;
        if let Some(task_id) = optional_arg(&input, "task_id") {
            if attempt.task_id.as_deref() != Some(task_id) {
                return Err(app_error(format!(
                    "PR attempt {attempt_id} is not attached to task {task_id}"
                )));
            }
        }
        Ok(json_output(pr_attempt_to_json(&attempt)))
    }
}

impl Tool for PrAttemptPreflightTool {
    fn name(&self) -> &str {
        "pr_attempt_preflight"
    }

    fn execute(&self, input: ToolInput) -> AppResult<ToolOutput> {
        let attempt_id =
            required_nonempty_any(&input, &["attempt_id", "id"], "pr_attempt_preflight")?;
        let attempt = self.load_attempt(&attempt_id)?;
        if let Some(task_id) = optional_arg(&input, "task_id") {
            if attempt.task_id.as_deref() != Some(task_id) {
                return Err(app_error(format!(
                    "PR attempt {attempt_id} is not attached to task {task_id}"
                )));
            }
        }
        let patch_path = PathBuf::from(&attempt.patch_path);
        ensure_attempt_patch_is_under_root(&self.root, &patch_path)?;
        let output = Command::new("git")
            .args(["apply", "--check"])
            .arg(&patch_path)
            .current_dir(&attempt.cwd)
            .output()?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        Ok(json_output(json_object([
            ("attempt_id", JsonValue::String(attempt.id)),
            ("patch_path", JsonValue::String(attempt.patch_path)),
            ("would_apply", JsonValue::Bool(output.status.success())),
            (
                "exit_code",
                output
                    .status
                    .code()
                    .map(|code| JsonValue::Number(code.to_string()))
                    .unwrap_or(JsonValue::Null),
            ),
            (
                "stdout_summary",
                JsonValue::String(clip_chars(stdout.trim(), 2_000)),
            ),
            (
                "stderr_summary",
                JsonValue::String(clip_chars(stderr.trim(), 2_000)),
            ),
            ("mutated_worktree", JsonValue::Bool(false)),
        ])))
    }
}

impl Tool for AutomationListTool {
    fn name(&self) -> &str {
        "automation_list"
    }

    fn execute(&self, input: ToolInput) -> AppResult<ToolOutput> {
        let limit = parse_limit(&input, 50, 100);
        let automations = self.store.list_automations(
            optional_arg(&input, "session_id"),
            optional_arg(&input, "thread_id"),
            limit,
        )?;
        let items = automations
            .iter()
            .map(automation_to_json)
            .collect::<Vec<_>>();
        Ok(json_output(json_object([
            (
                "summary",
                JsonValue::String(format!("{} automation(s)", items.len())),
            ),
            ("automations", json_array(items)),
        ])))
    }
}

impl Tool for AutomationReadTool {
    fn name(&self) -> &str {
        "automation_read"
    }

    fn execute(&self, input: ToolInput) -> AppResult<ToolOutput> {
        let automation_id =
            required_nonempty_any(&input, &["automation_id", "id"], "automation_read")?;
        let automation = self.store.load_automation(&automation_id)?;
        Ok(json_output(automation_to_json(&automation)))
    }
}

impl Tool for AutomationCreateTool {
    fn name(&self) -> &str {
        "automation_create"
    }

    fn execute(&self, input: ToolInput) -> AppResult<ToolOutput> {
        let name = required_nonempty_any(&input, &["name"], "automation_create")?;
        let prompt = required_nonempty_any(&input, &["prompt"], "automation_create")?;
        let schedule = required_nonempty_any(&input, &["rrule", "schedule"], "automation_create")?;
        let status = if let Some(status) = optional_arg(&input, "status") {
            status.to_string()
        } else if optional_bool_arg(&input, "paused") {
            "paused".to_string()
        } else {
            "active".to_string()
        };
        let automation = self.store.create_automation(
            optional_arg(&input, "session_id"),
            optional_arg(&input, "thread_id"),
            name,
            status,
            schedule,
            prompt,
            optional_arg(&input, "last_run_at").map(str::to_string),
            optional_arg(&input, "next_run_at").map(str::to_string),
        )?;
        Ok(json_output(automation_to_json(&automation)))
    }
}

impl Tool for AutomationRunTool {
    fn name(&self) -> &str {
        "automation_run"
    }

    fn execute(&self, input: ToolInput) -> AppResult<ToolOutput> {
        let automation_id =
            required_nonempty_any(&input, &["automation_id", "id"], "automation_run")?;
        let prompt_override = optional_arg(&input, "prompt")
            .or_else(|| optional_arg(&input, "prompt_override"))
            .map(str::to_string);
        let (automation, task) = self
            .store
            .trigger_automation(&automation_id, prompt_override)?;
        Ok(json_output(json_object([
            ("automation", automation_to_json(&automation)),
            ("task", task_to_json(&task)),
        ])))
    }
}

impl Tool for AutomationUpdateTool {
    fn name(&self) -> &str {
        "automation_update"
    }

    fn execute(&self, input: ToolInput) -> AppResult<ToolOutput> {
        let automation_id =
            required_nonempty_any(&input, &["automation_id", "id"], "automation_update")?;
        let status = optional_arg(&input, "status")
            .map(str::to_string)
            .or_else(|| optional_bool_value_arg(&input, "paused").map(paused_status));
        let schedule = optional_arg(&input, "rrule")
            .or_else(|| optional_arg(&input, "schedule"))
            .map(str::to_string);
        let automation = self.store.update_automation(
            &automation_id,
            optional_arg(&input, "name").map(str::to_string),
            status,
            schedule,
            optional_arg(&input, "prompt").map(str::to_string),
            optional_arg(&input, "next_run_at").map(str::to_string),
        )?;
        Ok(json_output(automation_to_json(&automation)))
    }
}

impl Tool for AutomationPauseTool {
    fn name(&self) -> &str {
        "automation_pause"
    }

    fn execute(&self, input: ToolInput) -> AppResult<ToolOutput> {
        let automation_id =
            required_nonempty_any(&input, &["automation_id", "id"], "automation_pause")?;
        Ok(json_output(automation_to_json(
            &self.store.pause_automation(&automation_id)?,
        )))
    }
}

impl Tool for AutomationResumeTool {
    fn name(&self) -> &str {
        "automation_resume"
    }

    fn execute(&self, input: ToolInput) -> AppResult<ToolOutput> {
        let automation_id =
            required_nonempty_any(&input, &["automation_id", "id"], "automation_resume")?;
        Ok(json_output(automation_to_json(
            &self.store.resume_automation(&automation_id)?,
        )))
    }
}

impl Tool for AutomationDeleteTool {
    fn name(&self) -> &str {
        "automation_delete"
    }

    fn execute(&self, input: ToolInput) -> AppResult<ToolOutput> {
        let automation_id =
            required_nonempty_any(&input, &["automation_id", "id"], "automation_delete")?;
        Ok(json_output(automation_to_json(
            &self.store.delete_automation(&automation_id)?,
        )))
    }
}

impl Tool for AgentSpawnTool {
    fn name(&self) -> &str {
        "agent_spawn"
    }

    fn execute(&self, input: ToolInput) -> AppResult<ToolOutput> {
        let prompt = required_nonempty_any(
            &input,
            &["prompt", "message", "objective", "task"],
            "agent_spawn",
        )?;
        let workspace = optional_arg(&input, "cwd")
            .or_else(|| optional_arg(&input, "workspace"))
            .map(str::to_string)
            .unwrap_or_else(default_workspace);
        let model = optional_arg(&input, "model")
            .map(str::to_string)
            .unwrap_or_else(|| self.config.model.model.clone());
        let mode = optional_arg(&input, "mode")
            .map(str::to_string)
            .unwrap_or_else(|| "agent".to_string());
        let title = optional_arg(&input, "title")
            .map(str::to_string)
            .unwrap_or_else(|| summarize_agent_prompt(&prompt));
        let thread = match optional_arg(&input, "thread_id") {
            Some(thread_id) => self.store.load_thread(thread_id)?,
            None => self.store.create_thread(title, workspace, model, mode)?,
        };
        let task = self.store.create_task(
            thread.session_id.as_deref(),
            Some(&thread.id),
            optional_arg(&input, "parent_task_id"),
            "subagent".to_string(),
            "pending".to_string(),
            prompt,
        )?;
        Ok(json_output(agent_snapshot_json(&self.store, &task)?))
    }
}

impl Tool for AgentResultTool {
    fn name(&self) -> &str {
        "agent_result"
    }

    fn execute(&self, input: ToolInput) -> AppResult<ToolOutput> {
        let agent_id = required_nonempty_any(&input, &["agent_id", "id"], "agent_result")?;
        let task = self.store.load_task(&agent_id)?;
        ensure_agent_task(&task, "agent_result")?;
        Ok(json_output(agent_snapshot_json(&self.store, &task)?))
    }
}

impl Tool for AgentListTool {
    fn name(&self) -> &str {
        "agent_list"
    }

    fn execute(&self, input: ToolInput) -> AppResult<ToolOutput> {
        let limit = parse_limit(&input, 20, 100);
        let agents = self
            .store
            .list_tasks(None, None, limit)?
            .into_iter()
            .filter(is_agent_task)
            .map(|task| agent_snapshot_json(&self.store, &task))
            .collect::<AppResult<Vec<_>>>()?;
        Ok(json_output(json_object([
            (
                "summary",
                JsonValue::String(format!("{} sub-agent(s)", agents.len())),
            ),
            ("agents", json_array(agents)),
        ])))
    }
}

impl Tool for AgentCancelTool {
    fn name(&self) -> &str {
        "agent_cancel"
    }

    fn execute(&self, input: ToolInput) -> AppResult<ToolOutput> {
        let agent_id = required_nonempty_any(&input, &["agent_id", "id"], "agent_cancel")?;
        let task = self.store.load_task(&agent_id)?;
        ensure_agent_task(&task, "agent_cancel")?;
        let (task, event) = self
            .store
            .cancel_task(&agent_id, "cancelled by agent_cancel".to_string())?;
        Ok(json_output(json_object([
            ("agent", agent_snapshot_json(&self.store, &task)?),
            (
                "event",
                event.as_ref().map(event_to_json).unwrap_or(JsonValue::Null),
            ),
        ])))
    }
}

impl Tool for AgentCloseTool {
    fn name(&self) -> &str {
        "close_agent"
    }

    fn execute(&self, input: ToolInput) -> AppResult<ToolOutput> {
        let agent_id = required_nonempty_any(&input, &["agent_id", "id"], "close_agent")?;
        let task = self.store.load_task(&agent_id)?;
        ensure_agent_task(&task, "close_agent")?;
        if matches!(task.status.as_str(), "completed" | "failed" | "cancelled") {
            return Ok(json_output(agent_snapshot_json(&self.store, &task)?));
        }
        let (task, _) = self
            .store
            .cancel_task(&agent_id, "closed by close_agent".to_string())?;
        Ok(json_output(agent_snapshot_json(&self.store, &task)?))
    }
}

impl Tool for AgentResumeTool {
    fn name(&self) -> &str {
        "resume_agent"
    }

    fn execute(&self, input: ToolInput) -> AppResult<ToolOutput> {
        let agent_id = required_nonempty_any(&input, &["agent_id", "id"], "resume_agent")?;
        let task = self.store.load_task(&agent_id)?;
        ensure_agent_task(&task, "resume_agent")?;
        let resumed = if task.status == "paused" {
            self.store.resume_task(
                &task.id,
                optional_arg(&input, "prompt")
                    .or_else(|| optional_arg(&input, "message"))
                    .map(str::to_string),
            )?
        } else {
            self.store.create_task(
                task.session_id.as_deref(),
                task.thread_id.as_deref(),
                Some(&task.id),
                "subagent".to_string(),
                "pending".to_string(),
                optional_arg(&input, "prompt")
                    .or_else(|| optional_arg(&input, "message"))
                    .map(str::to_string)
                    .unwrap_or_else(|| task.summary.clone()),
            )?
        };
        Ok(json_output(json_object([
            ("agent_id", JsonValue::String(resumed.id.clone())),
            ("agent", agent_snapshot_json(&self.store, &resumed)?),
            ("resumed_from", JsonValue::String(task.id)),
        ])))
    }
}

impl Tool for AgentSendInputTool {
    fn name(&self) -> &str {
        "send_input"
    }

    fn execute(&self, input: ToolInput) -> AppResult<ToolOutput> {
        let agent_id = required_nonempty_any(&input, &["agent_id", "id"], "send_input")?;
        let message = required_nonempty_any(&input, &["message", "input", "prompt"], "send_input")?;
        let task = self.store.load_task(&agent_id)?;
        ensure_agent_task(&task, "send_input")?;
        let thread_id = task
            .thread_id
            .clone()
            .ok_or_else(|| app_error("send_input requires an agent linked to a runtime thread"))?;
        let turn = self
            .store
            .append_turn(&thread_id, "user".to_string(), message.clone())?;
        let item = self.store.append_item(
            &thread_id,
            Some(&turn.id),
            "message".to_string(),
            Some("user".to_string()),
            message.clone(),
            "completed".to_string(),
        )?;
        let followup = self.store.create_task(
            task.session_id.as_deref(),
            Some(&thread_id),
            Some(&task.id),
            "subagent_input".to_string(),
            "pending".to_string(),
            message,
        )?;
        Ok(json_output(json_object([
            ("agent_id", JsonValue::String(task.id)),
            ("queued_agent_id", JsonValue::String(followup.id.clone())),
            ("queued_agent", agent_snapshot_json(&self.store, &followup)?),
            ("input_item", item_to_json(&item)),
        ])))
    }
}

fn runtime_store(config: &AppConfig) -> RuntimeStore {
    RuntimeStore::new(PathBuf::from(&config.workspace.config_dir).join("runtime"))
}

fn agent_snapshot_json(store: &RuntimeStore, task: &TaskRecord) -> AppResult<JsonValue> {
    let thread = task
        .thread_id
        .as_deref()
        .map(|thread_id| store.load_thread(thread_id))
        .transpose()?;
    let latest_item = match task.thread_id.as_deref() {
        Some(thread_id) => store
            .list_items(thread_id, None)?
            .into_iter()
            .rev()
            .find(|item| item.role.as_deref() == Some("assistant")),
        None => None,
    };
    Ok(json_object([
        ("agent_id", JsonValue::String(task.id.clone())),
        ("status", JsonValue::String(task.status.clone())),
        ("task", task_to_json(task)),
        (
            "thread",
            thread
                .as_ref()
                .map(thread_to_json)
                .unwrap_or(JsonValue::Null),
        ),
        (
            "result",
            latest_item
                .as_ref()
                .map(item_to_json)
                .unwrap_or(JsonValue::Null),
        ),
    ]))
}

fn ensure_agent_task(task: &TaskRecord, tool_name: &str) -> AppResult<()> {
    if is_agent_task(task) {
        Ok(())
    } else {
        Err(app_error(format!(
            "{tool_name} expected a sub-agent task id, got task kind `{}`",
            task.kind
        )))
    }
}

fn is_agent_task(task: &TaskRecord) -> bool {
    task.kind == "subagent" || task.kind == "subagent_input"
}

fn default_workspace() -> String {
    std::env::current_dir()
        .ok()
        .map(|path| path.display().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| ".".to_string())
}

fn summarize_agent_prompt(prompt: &str) -> String {
    let mut out = String::new();
    for (index, ch) in prompt.chars().enumerate() {
        if index >= 80 {
            out.push_str("...");
            break;
        }
        out.push(ch);
    }
    if out.trim().is_empty() {
        "Sub-agent task".to_string()
    } else {
        out
    }
}

fn pr_attempt_root(config: &AppConfig) -> PathBuf {
    PathBuf::from(&config.workspace.config_dir)
        .join("runtime")
        .join("pr_attempts")
}

fn json_output(value: JsonValue) -> ToolOutput {
    ToolOutput {
        summary: json_value_to_string(&value),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PrAttemptRecord {
    id: String,
    task_id: Option<String>,
    attempt_group_id: String,
    attempt_index: u32,
    attempt_count: u32,
    summary: String,
    verification: Vec<String>,
    changed_files: Vec<String>,
    base_sha: Option<String>,
    head_sha: Option<String>,
    branch: Option<String>,
    cwd: String,
    patch_path: String,
    recorded_at: String,
}

impl PrAttemptRecordTool {
    fn write_attempt(&self, attempt: &PrAttemptRecord) -> AppResult<()> {
        fs::create_dir_all(&self.root)?;
        let path = self.root.join(format!("{}.json", attempt.id));
        fs::write(path, json_value_to_string(&pr_attempt_to_json(attempt)))?;
        Ok(())
    }
}

impl PrAttemptListTool {
    fn list_attempts(&self, task_id: Option<&str>) -> AppResult<Vec<PrAttemptRecord>> {
        list_attempts_from_root(&self.root, task_id)
    }
}

impl PrAttemptReadTool {
    fn load_attempt(&self, attempt_id: &str) -> AppResult<PrAttemptRecord> {
        load_attempt_from_root(&self.root, attempt_id)
    }
}

impl PrAttemptPreflightTool {
    fn load_attempt(&self, attempt_id: &str) -> AppResult<PrAttemptRecord> {
        load_attempt_from_root(&self.root, attempt_id)
    }
}

fn list_attempts_from_root(root: &Path, task_id: Option<&str>) -> AppResult<Vec<PrAttemptRecord>> {
    if !root.exists() {
        return Ok(Vec::new());
    }
    let mut attempts = Vec::new();
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) != Some("json") {
            continue;
        }
        let content = fs::read_to_string(&path)?;
        let attempt = parse_pr_attempt_record(&parse_root_object(&content)?)?;
        if task_id.is_none() || attempt.task_id.as_deref() == task_id {
            attempts.push(attempt);
        }
    }
    attempts.sort_by(|left, right| right.recorded_at.cmp(&left.recorded_at));
    Ok(attempts)
}

fn load_attempt_from_root(root: &Path, attempt_id: &str) -> AppResult<PrAttemptRecord> {
    validate_simple_id(attempt_id, "attempt_id")?;
    let path = root.join(format!("{attempt_id}.json"));
    if !path.exists() {
        return Err(app_error(format!("PR attempt not found: {attempt_id}")));
    }
    let content = fs::read_to_string(path)?;
    parse_pr_attempt_record(&parse_root_object(&content)?)
}

fn pr_attempt_to_json(attempt: &PrAttemptRecord) -> JsonValue {
    json_object([
        ("id", JsonValue::String(attempt.id.clone())),
        (
            "task_id",
            attempt
                .task_id
                .clone()
                .map(JsonValue::String)
                .unwrap_or(JsonValue::Null),
        ),
        (
            "attempt_group_id",
            JsonValue::String(attempt.attempt_group_id.clone()),
        ),
        (
            "attempt_index",
            JsonValue::Number(attempt.attempt_index.to_string()),
        ),
        (
            "attempt_count",
            JsonValue::Number(attempt.attempt_count.to_string()),
        ),
        ("summary", JsonValue::String(attempt.summary.clone())),
        (
            "verification",
            json_array(
                attempt
                    .verification
                    .iter()
                    .cloned()
                    .map(JsonValue::String)
                    .collect(),
            ),
        ),
        (
            "changed_files",
            json_array(
                attempt
                    .changed_files
                    .iter()
                    .cloned()
                    .map(JsonValue::String)
                    .collect(),
            ),
        ),
        (
            "base_sha",
            attempt
                .base_sha
                .clone()
                .map(JsonValue::String)
                .unwrap_or(JsonValue::Null),
        ),
        (
            "head_sha",
            attempt
                .head_sha
                .clone()
                .map(JsonValue::String)
                .unwrap_or(JsonValue::Null),
        ),
        (
            "branch",
            attempt
                .branch
                .clone()
                .map(JsonValue::String)
                .unwrap_or(JsonValue::Null),
        ),
        ("cwd", JsonValue::String(attempt.cwd.clone())),
        ("patch_path", JsonValue::String(attempt.patch_path.clone())),
        (
            "recorded_at",
            JsonValue::String(attempt.recorded_at.clone()),
        ),
    ])
}

fn parse_pr_attempt_record(
    root: &std::collections::BTreeMap<String, JsonValue>,
) -> AppResult<PrAttemptRecord> {
    Ok(PrAttemptRecord {
        id: required_json_string(root, "id")?,
        task_id: optional_json_string(root, "task_id"),
        attempt_group_id: required_json_string(root, "attempt_group_id")?,
        attempt_index: optional_json_u64(root, "attempt_index").unwrap_or(1).max(1) as u32,
        attempt_count: optional_json_u64(root, "attempt_count").unwrap_or(1).max(1) as u32,
        summary: required_json_string(root, "summary")?,
        verification: string_array_field(root, "verification"),
        changed_files: string_array_field(root, "changed_files"),
        base_sha: optional_json_string(root, "base_sha"),
        head_sha: optional_json_string(root, "head_sha"),
        branch: optional_json_string(root, "branch"),
        cwd: required_json_string(root, "cwd")?,
        patch_path: required_json_string(root, "patch_path")?,
        recorded_at: required_json_string(root, "recorded_at")?,
    })
}

fn required_json_string(
    root: &std::collections::BTreeMap<String, JsonValue>,
    key: &str,
) -> AppResult<String> {
    root.get(key)
        .and_then(json_as_string)
        .filter(|value| !value.trim().is_empty())
        .map(str::to_string)
        .ok_or_else(|| app_error(format!("PR attempt record missing `{key}`")))
}

fn optional_json_string(
    root: &std::collections::BTreeMap<String, JsonValue>,
    key: &str,
) -> Option<String> {
    root.get(key)
        .and_then(json_as_string)
        .filter(|value| !value.trim().is_empty())
        .map(str::to_string)
}

fn optional_json_u64(
    root: &std::collections::BTreeMap<String, JsonValue>,
    key: &str,
) -> Option<u64> {
    root.get(key).and_then(json_as_u64)
}

fn string_array_field(
    root: &std::collections::BTreeMap<String, JsonValue>,
    key: &str,
) -> Vec<String> {
    root.get(key)
        .and_then(json_as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(json_as_string)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn required_nonempty_any(input: &ToolInput, keys: &[&str], tool_name: &str) -> AppResult<String> {
    keys.iter()
        .find_map(|key| {
            input
                .get(key)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
        })
        .ok_or_else(|| app_error(format!("{tool_name} requires `{}`", keys[0])))
}

fn optional_arg<'a>(input: &'a ToolInput, key: &str) -> Option<&'a str> {
    input
        .get(key)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn optional_bool_arg(input: &ToolInput, key: &str) -> bool {
    optional_bool_value_arg(input, key).unwrap_or(false)
}

fn optional_bool_value_arg(input: &ToolInput, key: &str) -> Option<bool> {
    optional_arg(input, key).map(|value| {
        matches!(
            value.to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        )
    })
}

fn paused_status(paused: bool) -> String {
    if paused { "paused" } else { "active" }.to_string()
}

fn parse_limit(input: &ToolInput, default: usize, max: usize) -> usize {
    input
        .get("limit")
        .and_then(|value| value.trim().parse::<usize>().ok())
        .unwrap_or(default)
        .clamp(1, max)
}

fn parse_u32_arg(input: &ToolInput, key: &str, default: u32) -> u32 {
    input
        .get(key)
        .and_then(|value| value.trim().parse::<u32>().ok())
        .unwrap_or(default)
}

fn parse_verification(raw: Option<&str>) -> AppResult<Vec<String>> {
    let Some(raw) = raw.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(Vec::new());
    };
    if raw.starts_with('[') {
        let JsonValue::Array(items) = parse_json_value(raw)? else {
            return Err(app_error("verification must be a JSON array of strings"));
        };
        return Ok(items
            .iter()
            .filter_map(json_as_string)
            .map(str::to_string)
            .collect());
    }
    Ok(raw
        .split(|ch| ch == '\n' || ch == ';')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .collect())
}

fn run_git_capture(cwd: &str, args: &[&str]) -> AppResult<String> {
    let output = Command::new("git").current_dir(cwd).args(args).output()?;
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    if !output.status.success() {
        let exit_code = output.status.code().unwrap_or(-1);
        return Err(app_error(format!(
            "git {} failed with exit code {exit_code}: {}",
            args.join(" "),
            first_non_empty_line(&stderr)
                .or_else(|| first_non_empty_line(&stdout))
                .unwrap_or("no output")
        )));
    }
    Ok(stdout)
}

fn git_lines(cwd: &str, args: &[&str]) -> AppResult<Vec<String>> {
    Ok(run_git_capture(cwd, args)?
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect())
}

fn optional_git_output(cwd: &str, args: &[&str]) -> Option<String> {
    run_git_capture(cwd, args)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn first_non_empty_line(value: &str) -> Option<&str> {
    value.lines().map(str::trim).find(|line| !line.is_empty())
}

fn clip_chars(value: &str, max_chars: usize) -> String {
    let mut out = String::new();
    for (index, ch) in value.chars().enumerate() {
        if index >= max_chars {
            out.push_str("...");
            return out;
        }
        out.push(ch);
    }
    out
}

fn new_attempt_id() -> String {
    format!("attempt_{}", monotonic_suffix())
}

fn new_attempt_group_id() -> String {
    format!("attempt_group_{}", monotonic_suffix())
}

fn monotonic_suffix() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("{}_{nanos}", std::process::id())
}

fn epoch_label() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    format!("epoch+{secs}")
}

fn validate_simple_id(value: &str, label: &str) -> AppResult<()> {
    if value.is_empty()
        || value.len() > 128
        || !value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.'))
    {
        return Err(app_error(format!("{label} contains unsafe characters")));
    }
    Ok(())
}

fn ensure_attempt_patch_is_under_root(root: &Path, patch_path: &Path) -> AppResult<()> {
    let root = root.canonicalize()?;
    let patch = patch_path.canonicalize()?;
    if !patch.starts_with(&root) {
        return Err(app_error(
            "PR attempt patch path is outside the attempt store",
        ));
    }
    Ok(())
}

fn validate_gate(gate: &str) -> AppResult<()> {
    match gate {
        "fmt" | "check" | "clippy" | "test" | "custom" => Ok(()),
        _ => Err(app_error(
            "task_gate_run gate must be one of fmt, check, clippy, test, or custom",
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_root(name: &str) -> PathBuf {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "deepseek-runtime-tools-{name}-{}-{suffix}",
            std::process::id()
        ))
    }

    fn config_for(root: &std::path::Path) -> AppConfig {
        let mut config = AppConfig::default();
        config.workspace.config_dir = root.join(".dscode").display().to_string();
        config
    }

    #[test]
    fn task_create_list_read_and_cancel_round_trip() {
        let root = temp_root("task-roundtrip");
        let config = config_for(&root);
        let create = TaskCreateTool::new(&config);
        let list = TaskListTool::new(&config);
        let read = TaskReadTool::new(&config);
        let cancel = TaskCancelTool::new(&config);

        let created = create
            .execute(ToolInput::new().with_arg("prompt", "verify runtime tools"))
            .unwrap();
        assert!(created
            .summary
            .contains("\"summary\":\"verify runtime tools\""));
        let task_id = extract_json_string(&created.summary, "id").unwrap();

        let listed = list.execute(ToolInput::new()).unwrap();
        assert!(listed.summary.contains(&task_id));

        let read_back = read
            .execute(ToolInput::new().with_arg("task_id", &task_id))
            .unwrap();
        assert!(read_back.summary.contains("\"status\":\"pending\""));

        let cancelled = cancel
            .execute(
                ToolInput::new()
                    .with_arg("task_id", &task_id)
                    .with_arg("reason", "test cancel"),
            )
            .unwrap();
        assert!(cancelled.summary.contains("\"status\":\"cancelled\""));

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn automation_list_and_read_round_trip() {
        let root = temp_root("automation-roundtrip");
        let config = config_for(&root);
        let store = runtime_store(&config);
        let automation = store
            .create_automation(
                None,
                None,
                "nightly".to_string(),
                "active".to_string(),
                "daily".to_string(),
                "run checks".to_string(),
                None,
                Some("999".to_string()),
            )
            .unwrap();

        let listed = AutomationListTool::new(&config)
            .execute(ToolInput::new())
            .unwrap();
        assert!(listed.summary.contains(&automation.id));

        let read = AutomationReadTool::new(&config)
            .execute(ToolInput::new().with_arg("automation_id", &automation.id))
            .unwrap();
        assert!(read.summary.contains("\"name\":\"nightly\""));
        assert!(read.summary.contains("\"prompt\":\"run checks\""));

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn automation_create_list_read_and_run_round_trip() {
        let root = temp_root("automation-create-run-roundtrip");
        let config = config_for(&root);
        let create = AutomationCreateTool::new(&config);
        let list = AutomationListTool::new(&config);
        let read = AutomationReadTool::new(&config);
        let run = AutomationRunTool::new(&config);

        let created = create
            .execute(
                ToolInput::new()
                    .with_arg("name", "weekly review")
                    .with_arg("prompt", "summarize open work")
                    .with_arg("rrule", "FREQ=WEEKLY;BYDAY=MO;BYHOUR=9;BYMINUTE=30")
                    .with_arg("next_run_at", "999"),
            )
            .unwrap();
        assert!(created.summary.contains("\"name\":\"weekly review\""));
        assert!(created.summary.contains("\"status\":\"active\""));
        assert!(created
            .summary
            .contains("\"prompt\":\"summarize open work\""));
        let automation_id = extract_json_string(&created.summary, "id").unwrap();

        let listed = list.execute(ToolInput::new()).unwrap();
        assert!(listed.summary.contains(&automation_id));

        let read_back = read
            .execute(ToolInput::new().with_arg("automation_id", &automation_id))
            .unwrap();
        assert!(read_back.summary.contains("FREQ=WEEKLY"));

        let run_now = run
            .execute(
                ToolInput::new()
                    .with_arg("automation_id", &automation_id)
                    .with_arg("prompt_override", "use this run prompt"),
            )
            .unwrap();
        assert!(run_now.summary.contains("\"kind\":\"automation\""));
        assert!(run_now
            .summary
            .contains("\"summary\":\"use this run prompt\""));
        assert!(run_now.summary.contains("\"last_run_at\":"));

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn automation_create_accepts_paused_flag_and_schedule_alias() {
        let root = temp_root("automation-create-paused");
        let config = config_for(&root);
        let created = AutomationCreateTool::new(&config)
            .execute(
                ToolInput::new()
                    .with_arg("name", "manual")
                    .with_arg("prompt", "run later")
                    .with_arg("schedule", "manual")
                    .with_arg("paused", "true"),
            )
            .unwrap();
        assert!(created.summary.contains("\"status\":\"paused\""));
        assert!(created.summary.contains("\"schedule\":\"manual\""));

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn automation_update_pause_resume_and_delete_round_trip() {
        let root = temp_root("automation-lifecycle-tools");
        let config = config_for(&root);
        let created = AutomationCreateTool::new(&config)
            .execute(
                ToolInput::new()
                    .with_arg("name", "nightly")
                    .with_arg("prompt", "run checks")
                    .with_arg("rrule", "FREQ=HOURLY;INTERVAL=1"),
            )
            .unwrap();
        let automation_id = extract_json_string(&created.summary, "id").unwrap();

        let updated = AutomationUpdateTool::new(&config)
            .execute(
                ToolInput::new()
                    .with_arg("automation_id", &automation_id)
                    .with_arg("name", "weekly")
                    .with_arg("prompt", "summarize work")
                    .with_arg("rrule", "FREQ=WEEKLY;BYDAY=FR")
                    .with_arg("paused", "true"),
            )
            .unwrap();
        assert!(updated.summary.contains("\"name\":\"weekly\""));
        assert!(updated.summary.contains("\"status\":\"paused\""));
        assert!(updated.summary.contains("FREQ=WEEKLY"));

        let resumed = AutomationResumeTool::new(&config)
            .execute(ToolInput::new().with_arg("automation_id", &automation_id))
            .unwrap();
        assert!(resumed.summary.contains("\"status\":\"active\""));

        let paused = AutomationPauseTool::new(&config)
            .execute(ToolInput::new().with_arg("id", &automation_id))
            .unwrap();
        assert!(paused.summary.contains("\"status\":\"paused\""));

        let deleted = AutomationDeleteTool::new(&config)
            .execute(ToolInput::new().with_arg("automation_id", &automation_id))
            .unwrap();
        assert!(deleted.summary.contains("\"status\":\"cancelled\""));
        assert!(AutomationReadTool::new(&config)
            .execute(ToolInput::new().with_arg("automation_id", &automation_id))
            .is_err());

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn agent_lifecycle_tools_use_runtime_subagent_tasks() {
        let root = temp_root("agent-lifecycle");
        let config = config_for(&root);
        let spawn = AgentSpawnTool::new(&config)
            .execute(
                ToolInput::new()
                    .with_arg("prompt", "inspect cache behavior")
                    .with_arg("cwd", root.display().to_string())
                    .with_arg("model", "deepseek-coder"),
            )
            .unwrap();
        assert!(spawn.summary.contains("\"agent_id\":\"task-"));
        assert!(spawn.summary.contains("\"kind\":\"subagent\""));
        assert!(spawn.summary.contains("\"status\":\"pending\""));
        let agent_id = extract_json_string(&spawn.summary, "agent_id").unwrap();

        let result = AgentResultTool::new(&config)
            .execute(ToolInput::new().with_arg("agent_id", &agent_id))
            .unwrap();
        assert!(result.summary.contains("\"agent_id\":\"task-"));
        assert!(result.summary.contains("\"result\":null"));

        let listed = AgentListTool::new(&config)
            .execute(ToolInput::new())
            .unwrap();
        assert!(listed.summary.contains(&agent_id));

        let sent = AgentSendInputTool::new(&config)
            .execute(
                ToolInput::new()
                    .with_arg("agent_id", &agent_id)
                    .with_arg("message", "also inspect prefix caching"),
            )
            .unwrap();
        assert!(sent.summary.contains("\"queued_agent_id\":\"task-"));
        assert!(sent.summary.contains("\"kind\":\"subagent_input\""));
        assert!(sent.summary.contains("prefix caching"));

        let cancelled = AgentCancelTool::new(&config)
            .execute(ToolInput::new().with_arg("agent_id", &agent_id))
            .unwrap();
        assert!(cancelled.summary.contains("\"status\":\"cancelled\""));

        let closed = AgentCloseTool::new(&config)
            .execute(ToolInput::new().with_arg("id", &agent_id))
            .unwrap();
        assert!(closed.summary.contains("\"status\":\"cancelled\""));

        let resumed = AgentResumeTool::new(&config)
            .execute(ToolInput::new().with_arg("agent_id", &agent_id))
            .unwrap();
        assert!(resumed.summary.contains("\"resumed_from\""));
        assert!(resumed.summary.contains("\"status\":\"pending\""));

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn pr_attempt_record_list_read_and_preflight_round_trip() {
        if !git_available() {
            return;
        }
        let root = temp_root("pr-attempt-roundtrip");
        let repo = root.join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        init_git_repo(&repo);
        std::fs::write(repo.join("file.txt"), "changed\n").unwrap();

        let config = config_for(&root);
        let record = PrAttemptRecordTool::new(&config)
            .execute(
                ToolInput::new()
                    .with_arg("summary", "try changed file")
                    .with_arg("task_id", "task_demo")
                    .with_arg("verification", r#"["cargo test"]"#)
                    .with_arg("cwd", repo.display().to_string()),
            )
            .unwrap();
        assert!(record
            .summary
            .contains("\"summary\":\"PR attempt recorded\""));
        assert!(record.summary.contains("\"changed_files\":[\"file.txt\"]"));
        assert!(record.summary.contains("\"cargo test\""));
        let attempt_id = extract_json_string(&record.summary, "id").unwrap();

        let listed = PrAttemptListTool::new(&config)
            .execute(ToolInput::new().with_arg("task_id", "task_demo"))
            .unwrap();
        assert!(listed.summary.contains(&attempt_id));

        let read = PrAttemptReadTool::new(&config)
            .execute(ToolInput::new().with_arg("attempt_id", &attempt_id))
            .unwrap();
        assert!(read.summary.contains("\"patch_path\""));

        run_git_test(&repo, &["checkout", "--", "file.txt"]);
        let preflight = PrAttemptPreflightTool::new(&config)
            .execute(ToolInput::new().with_arg("attempt_id", &attempt_id))
            .unwrap();
        assert!(preflight.summary.contains("\"would_apply\":true"));
        assert!(preflight.summary.contains("\"mutated_worktree\":false"));

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn pr_attempt_record_rejects_empty_diff() {
        if !git_available() {
            return;
        }
        let root = temp_root("pr-attempt-empty");
        let repo = root.join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        init_git_repo(&repo);

        let config = config_for(&root);
        let error = PrAttemptRecordTool::new(&config)
            .execute(
                ToolInput::new()
                    .with_arg("summary", "empty")
                    .with_arg("cwd", repo.display().to_string()),
            )
            .unwrap_err();
        assert!(error.to_string().contains("No working-tree diff"));

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn task_gate_run_delegates_to_safe_shell() {
        let output = TaskGateRunTool
            .execute(
                ToolInput::new()
                    .with_arg("gate", "test")
                    .with_arg("command", "echo gate-ok"),
            )
            .unwrap();

        assert!(output.summary.contains("meta.gate=test"));
        assert!(output.summary.contains("meta.command=echo gate-ok"));
        assert!(output.summary.contains("gate-ok"));
    }

    #[test]
    fn task_gate_run_rejects_unknown_gate() {
        let error = TaskGateRunTool
            .execute(
                ToolInput::new()
                    .with_arg("gate", "deploy")
                    .with_arg("command", "printf nope"),
            )
            .unwrap_err();

        assert!(error.to_string().contains("gate must be one of"));
    }

    fn extract_json_string(input: &str, key: &str) -> Option<String> {
        let marker = format!("\"{key}\":\"");
        let rest = input.split_once(&marker)?.1;
        Some(rest.split_once('"')?.0.to_string())
    }

    fn git_available() -> bool {
        std::process::Command::new("git")
            .arg("--version")
            .output()
            .map(|output| output.status.success())
            .unwrap_or(false)
    }

    fn init_git_repo(repo: &std::path::Path) {
        run_git_test(repo, &["init"]);
        run_git_test(repo, &["config", "user.email", "test@example.com"]);
        run_git_test(repo, &["config", "user.name", "Test User"]);
        std::fs::write(repo.join("file.txt"), "base\n").unwrap();
        run_git_test(repo, &["add", "file.txt"]);
        run_git_test(repo, &["commit", "-m", "initial"]);
    }

    fn run_git_test(repo: &std::path::Path, args: &[&str]) {
        let output = std::process::Command::new("git")
            .current_dir(repo)
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {:?} failed: stdout={} stderr={}",
            args,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
