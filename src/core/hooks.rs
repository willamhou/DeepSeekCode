use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::config::types::HooksConfig;
use crate::error::{app_error, policy_denied, AppResult};
use crate::tools::types::ToolInput;
use crate::util::json::{json_as_string, json_value_to_string, parse_root_object, JsonValue};

const HOOK_OUTPUT_LIMIT: usize = 16 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HookEvent {
    SessionStart,
    SessionStop,
    UserPromptSubmit,
    PreToolUse,
    PermissionRequest,
    PostToolUse,
    SubagentStart,
    SubagentStop,
    PreCompact,
}

impl HookEvent {
    fn dir_name(self) -> &'static str {
        match self {
            Self::SessionStart => "session_start",
            Self::SessionStop => "session_stop",
            Self::UserPromptSubmit => "user_prompt_submit",
            Self::PreToolUse => "pre_tool_use",
            Self::PermissionRequest => "permission_request",
            Self::PostToolUse => "post_tool_use",
            Self::SubagentStart => "subagent_start",
            Self::SubagentStop => "subagent_stop",
            Self::PreCompact => "pre_compact",
        }
    }

    fn blocks_on_failure(self) -> bool {
        matches!(
            self,
            Self::UserPromptSubmit | Self::PreToolUse | Self::PermissionRequest
        )
    }
}

#[derive(Debug, Clone)]
pub struct HookRunner {
    enabled: bool,
    project_dir: PathBuf,
    user_dir: PathBuf,
    timeout_ms: u64,
}

impl HookRunner {
    pub fn new(config: &HooksConfig) -> Self {
        Self {
            enabled: config.enabled,
            project_dir: PathBuf::from(&config.project_dir),
            user_dir: crate::skills::tilde::expand_tilde(&config.user_dir),
            timeout_ms: config.timeout_ms.max(1),
        }
    }

    pub fn user_prompt_submit(&self, task: &str) -> AppResult<Option<String>> {
        self.run(
            HookEvent::UserPromptSubmit,
            hook_payload(HookPayload {
                event: HookEvent::UserPromptSubmit,
                task,
                tool_name: None,
                tool_input: None,
                tool_status: None,
                tool_output: None,
                metadata: BTreeMap::new(),
            }),
        )
    }

    pub fn session_start(&self, task: &str, reason: &str) -> AppResult<Option<String>> {
        self.run(
            HookEvent::SessionStart,
            hook_payload(HookPayload {
                event: HookEvent::SessionStart,
                task,
                tool_name: None,
                tool_input: None,
                tool_status: None,
                tool_output: None,
                metadata: BTreeMap::from([("reason".to_string(), reason.to_string())]),
            }),
        )
    }

    pub fn session_stop(
        &self,
        task: &str,
        reason: &str,
        final_message: &str,
    ) -> AppResult<Option<String>> {
        self.run(
            HookEvent::SessionStop,
            hook_payload(HookPayload {
                event: HookEvent::SessionStop,
                task,
                tool_name: None,
                tool_input: None,
                tool_status: None,
                tool_output: Some(final_message),
                metadata: BTreeMap::from([("reason".to_string(), reason.to_string())]),
            }),
        )
    }

    pub fn pre_tool_use(
        &self,
        task: &str,
        tool_name: &str,
        input: &ToolInput,
    ) -> AppResult<Option<String>> {
        self.run(
            HookEvent::PreToolUse,
            hook_payload(HookPayload {
                event: HookEvent::PreToolUse,
                task,
                tool_name: Some(tool_name),
                tool_input: Some(input),
                tool_status: None,
                tool_output: None,
                metadata: BTreeMap::new(),
            }),
        )
    }

    pub fn permission_request(
        &self,
        task: &str,
        tool_name: &str,
        input: &ToolInput,
        permission_kind: &str,
        permission_target: &str,
    ) -> AppResult<Option<String>> {
        self.run(
            HookEvent::PermissionRequest,
            hook_payload(HookPayload {
                event: HookEvent::PermissionRequest,
                task,
                tool_name: Some(tool_name),
                tool_input: Some(input),
                tool_status: None,
                tool_output: None,
                metadata: BTreeMap::from([
                    ("permission_kind".to_string(), permission_kind.to_string()),
                    (
                        "permission_target".to_string(),
                        permission_target.to_string(),
                    ),
                ]),
            }),
        )
    }

    pub fn post_tool_use(
        &self,
        task: &str,
        tool_name: &str,
        input: &BTreeMap<String, String>,
        status: crate::model::protocol::ObservationStatus,
        output: &str,
    ) -> AppResult<Option<String>> {
        self.run(
            HookEvent::PostToolUse,
            hook_payload(HookPayload {
                event: HookEvent::PostToolUse,
                task,
                tool_name: Some(tool_name),
                tool_input: Some(&ToolInput {
                    args: input.clone(),
                }),
                tool_status: Some(match status {
                    crate::model::protocol::ObservationStatus::Ok => "ok",
                    crate::model::protocol::ObservationStatus::Failed => "failed",
                }),
                tool_output: Some(output),
                metadata: BTreeMap::new(),
            }),
        )
    }

    pub fn subagent_start(
        &self,
        task: &str,
        subagent_task: &str,
        agent_name: Option<&str>,
    ) -> AppResult<Option<String>> {
        let mut metadata =
            BTreeMap::from([("subagent_task".to_string(), subagent_task.to_string())]);
        if let Some(agent_name) = agent_name {
            metadata.insert("agent".to_string(), agent_name.to_string());
        }
        self.run(
            HookEvent::SubagentStart,
            hook_payload(HookPayload {
                event: HookEvent::SubagentStart,
                task,
                tool_name: Some("dispatch_subagent"),
                tool_input: None,
                tool_status: None,
                tool_output: None,
                metadata,
            }),
        )
    }

    pub fn subagent_stop(
        &self,
        task: &str,
        subagent_task: &str,
        agent_name: Option<&str>,
        output: &str,
    ) -> AppResult<Option<String>> {
        let mut metadata =
            BTreeMap::from([("subagent_task".to_string(), subagent_task.to_string())]);
        if let Some(agent_name) = agent_name {
            metadata.insert("agent".to_string(), agent_name.to_string());
        }
        self.run(
            HookEvent::SubagentStop,
            hook_payload(HookPayload {
                event: HookEvent::SubagentStop,
                task,
                tool_name: Some("dispatch_subagent"),
                tool_input: None,
                tool_status: None,
                tool_output: Some(output),
                metadata,
            }),
        )
    }

    pub fn pre_compact(&self, task: &str, reason: &str) -> AppResult<Option<String>> {
        self.run(
            HookEvent::PreCompact,
            hook_payload(HookPayload {
                event: HookEvent::PreCompact,
                task,
                tool_name: None,
                tool_input: None,
                tool_status: None,
                tool_output: None,
                metadata: BTreeMap::from([("reason".to_string(), reason.to_string())]),
            }),
        )
    }

    fn run(&self, event: HookEvent, payload: String) -> AppResult<Option<String>> {
        if !self.enabled {
            return Ok(None);
        }

        let scripts = self.scripts_for(event)?;
        if scripts.is_empty() {
            return Ok(None);
        }

        let mut context = Vec::new();
        for script in scripts {
            let result = run_hook_script(&script, event, &payload, self.timeout_ms)?;
            if !result.success {
                let message = result.error_message(&script);
                if event.blocks_on_failure() {
                    return Err(policy_denied(message));
                }
                context.push(message);
                continue;
            }
            if !result.stdout.trim().is_empty() {
                match hook_stdout_decision(event, result.stdout.trim())? {
                    HookStdoutDecision::Allow(Some(add_context)) => context.push(format!(
                        "{}: {}",
                        script.display(),
                        truncate_output(&add_context)
                    )),
                    HookStdoutDecision::Allow(None) => {}
                    HookStdoutDecision::Deny(reason) => {
                        let message = format!(
                            "hook `{}` denied: {}",
                            script.display(),
                            truncate_output(&reason)
                        );
                        if event.blocks_on_failure() {
                            return Err(policy_denied(message));
                        }
                        context.push(message);
                    }
                }
            }
        }

        if context.is_empty() {
            Ok(None)
        } else {
            Ok(Some(context.join("\n")))
        }
    }

    fn scripts_for(&self, event: HookEvent) -> AppResult<Vec<PathBuf>> {
        let mut scripts = Vec::new();
        let mut user_scripts = Vec::new();
        collect_scripts(&self.user_dir.join(event.dir_name()), &mut user_scripts)?;
        user_scripts.sort();
        scripts.extend(user_scripts);

        let mut project_scripts = Vec::new();
        collect_scripts(
            &self.project_dir.join(event.dir_name()),
            &mut project_scripts,
        )?;
        project_scripts.sort();
        scripts.extend(project_scripts);
        Ok(scripts)
    }
}

struct HookPayload<'a> {
    event: HookEvent,
    task: &'a str,
    tool_name: Option<&'a str>,
    tool_input: Option<&'a ToolInput>,
    tool_status: Option<&'a str>,
    tool_output: Option<&'a str>,
    metadata: BTreeMap<String, String>,
}

fn hook_payload(payload: HookPayload<'_>) -> String {
    let mut root = BTreeMap::new();
    root.insert(
        "event".to_string(),
        JsonValue::String(payload.event.dir_name().to_string()),
    );
    root.insert(
        "task".to_string(),
        JsonValue::String(payload.task.to_string()),
    );
    root.insert(
        "tool_name".to_string(),
        payload
            .tool_name
            .map(|value| JsonValue::String(value.to_string()))
            .unwrap_or(JsonValue::Null),
    );
    root.insert(
        "tool_input".to_string(),
        payload
            .tool_input
            .map(tool_input_json)
            .unwrap_or(JsonValue::Null),
    );
    root.insert(
        "tool_status".to_string(),
        payload
            .tool_status
            .map(|value| JsonValue::String(value.to_string()))
            .unwrap_or(JsonValue::Null),
    );
    root.insert(
        "tool_output".to_string(),
        payload
            .tool_output
            .map(|value| JsonValue::String(truncate_output(value)))
            .unwrap_or(JsonValue::Null),
    );
    root.insert(
        "metadata".to_string(),
        JsonValue::Object(
            payload
                .metadata
                .into_iter()
                .map(|(key, value)| (key, JsonValue::String(value)))
                .collect(),
        ),
    );
    json_value_to_string(&JsonValue::Object(root))
}

enum HookStdoutDecision {
    Allow(Option<String>),
    Deny(String),
}

fn hook_stdout_decision(event: HookEvent, stdout: &str) -> AppResult<HookStdoutDecision> {
    let trimmed = stdout.trim();
    if !trimmed.starts_with('{') {
        return Ok(HookStdoutDecision::Allow(Some(trimmed.to_string())));
    }
    let root = parse_root_object(trimmed)?;
    let decision = root.get("decision").and_then(json_as_string);
    let add_context = root
        .get("add_context")
        .or_else(|| root.get("additional_context"))
        .and_then(json_as_string)
        .map(str::to_string);
    let system_message = root
        .get("system_message")
        .or_else(|| root.get("systemMessage"))
        .and_then(json_as_string)
        .map(str::to_string);
    let context = add_context.or(system_message);

    match decision {
        Some("deny") | Some("block") => {
            let reason = root
                .get("reason")
                .or_else(|| root.get("message"))
                .and_then(json_as_string)
                .unwrap_or_else(|| {
                    if event.blocks_on_failure() {
                        "blocked by hook"
                    } else {
                        "advisory hook block"
                    }
                });
            Ok(HookStdoutDecision::Deny(reason.to_string()))
        }
        Some("allow") | None => Ok(HookStdoutDecision::Allow(context)),
        Some(other) => Err(app_error(format!("unsupported hook decision `{other}`"))),
    }
}

fn tool_input_json(input: &ToolInput) -> JsonValue {
    JsonValue::Object(
        input
            .args
            .iter()
            .map(|(key, value)| (key.clone(), JsonValue::String(value.clone())))
            .collect(),
    )
}

fn collect_scripts(dir: &Path, scripts: &mut Vec<PathBuf>) -> AppResult<()> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Ok(());
    };

    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        if is_executable_file(&path) {
            scripts.push(path);
        }
    }

    Ok(())
}

#[cfg(unix)]
fn is_executable_file(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    path.is_file()
        && path
            .metadata()
            .map(|metadata| metadata.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
}

#[cfg(not(unix))]
fn is_executable_file(path: &Path) -> bool {
    path.is_file()
}

#[derive(Debug)]
struct HookRunResult {
    stdout: String,
    stderr: String,
    success: bool,
    timed_out: bool,
}

impl HookRunResult {
    fn error_message(&self, script: &Path) -> String {
        if self.timed_out {
            return format!(
                "hook `{}` timed out after execution budget",
                script.display()
            );
        }
        let detail = if !self.stderr.trim().is_empty() {
            self.stderr.trim()
        } else if !self.stdout.trim().is_empty() {
            self.stdout.trim()
        } else {
            "hook exited unsuccessfully"
        };
        format!(
            "hook `{}` failed: {}",
            script.display(),
            truncate_output(detail)
        )
    }
}

fn run_hook_script(
    path: &Path,
    event: HookEvent,
    payload: &str,
    timeout_ms: u64,
) -> AppResult<HookRunResult> {
    let mut child = spawn_hook_process(path, event)?;
    let stdout_reader = spawn_output_reader(child.stdout.take());
    let stderr_reader = spawn_output_reader(child.stderr.take());
    let mut stdin_error = None;

    if let Some(mut stdin) = child.stdin.take() {
        if let Err(error) = stdin.write_all(payload.as_bytes()) {
            if error.kind() != std::io::ErrorKind::BrokenPipe {
                stdin_error = Some(error.to_string());
            }
        }
    }

    let deadline = Instant::now() + Duration::from_millis(timeout_ms);
    let mut timed_out = false;
    let status = loop {
        if let Some(status) = child.try_wait().map_err(|error| {
            app_error(format!("failed to poll hook `{}`: {error}", path.display()))
        })? {
            break status;
        }
        if Instant::now() >= deadline {
            timed_out = true;
            let _ = child.kill();
            break child.wait().map_err(|error| {
                app_error(format!(
                    "failed to await hook `{}`: {error}",
                    path.display()
                ))
            })?;
        }
        std::thread::sleep(Duration::from_millis(10));
    };

    let stdout = join_output_reader(stdout_reader, path, "stdout")?;
    let stderr = join_output_reader(stderr_reader, path, "stderr")?;

    if let Some(error) = stdin_error {
        return Err(app_error(format!("could not write hook stdin: {error}")));
    }

    Ok(HookRunResult {
        stdout,
        stderr,
        success: status.success() && !timed_out,
        timed_out,
    })
}

fn spawn_hook_process(path: &Path, event: HookEvent) -> AppResult<std::process::Child> {
    let mut last_error = None;
    for _ in 0..5 {
        match Command::new(path)
            .env("DSCODE_HOOK_EVENT", event.dir_name())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
        {
            Ok(child) => return Ok(child),
            Err(error) if error.raw_os_error() == Some(26) => {
                last_error = Some(error);
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(error) => {
                return Err(app_error(format!(
                    "could not invoke hook `{}`: {error}",
                    path.display()
                )));
            }
        }
    }

    let error = last_error
        .map(|error| error.to_string())
        .unwrap_or_else(|| "unknown spawn failure".to_string());
    Err(app_error(format!(
        "could not invoke hook `{}`: {error}",
        path.display()
    )))
}

fn spawn_output_reader<R>(reader: Option<R>) -> std::thread::JoinHandle<Result<String, String>>
where
    R: Read + Send + 'static,
{
    std::thread::spawn(move || read_limited(reader))
}

fn join_output_reader(
    handle: std::thread::JoinHandle<Result<String, String>>,
    path: &Path,
    stream_name: &str,
) -> AppResult<String> {
    handle
        .join()
        .map_err(|_| {
            app_error(format!(
                "hook `{}` {stream_name} reader panicked",
                path.display()
            ))
        })?
        .map_err(|error| {
            app_error(format!(
                "could not read hook `{}` {stream_name}: {error}",
                path.display(),
            ))
        })
}

fn read_limited<R: Read>(reader: Option<R>) -> Result<String, String> {
    let Some(mut reader) = reader else {
        return Ok(String::new());
    };
    let mut rolling = Vec::new();
    let mut chunk = [0_u8; 4096];
    loop {
        let read = reader
            .read(&mut chunk)
            .map_err(|error| format!("could not read hook output: {error}"))?;
        if read == 0 {
            break;
        }
        rolling.extend_from_slice(&chunk[..read]);
        if rolling.len() > HOOK_OUTPUT_LIMIT {
            let excess = rolling.len() - HOOK_OUTPUT_LIMIT;
            rolling.drain(0..excess);
        }
    }
    Ok(String::from_utf8_lossy(&rolling).to_string())
}

fn truncate_output(value: &str) -> String {
    if value.len() <= HOOK_OUTPUT_LIMIT {
        return value.to_string();
    }
    let mut start = value.len() - HOOK_OUTPUT_LIMIT;
    while !value.is_char_boundary(start) {
        start += 1;
    }
    value[start..].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root(name: &str) -> PathBuf {
        let suffix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "deepseek-hooks-{name}-{}-{suffix}",
            std::process::id()
        ))
    }

    #[cfg(unix)]
    fn write_hook(path: &Path, body: &str) {
        use std::os::unix::fs::PermissionsExt;
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, format!("#!/bin/sh\n{body}\n")).unwrap();
        let mut permissions = std::fs::metadata(path).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(path, permissions).unwrap();
    }

    #[test]
    fn hook_payload_includes_tool_fields_as_json() {
        let input = ToolInput::new().with_arg("path", "src/main.rs");
        let payload = hook_payload(HookPayload {
            event: HookEvent::PreToolUse,
            task: "inspect",
            tool_name: Some("read_file"),
            tool_input: Some(&input),
            tool_status: None,
            tool_output: None,
            metadata: BTreeMap::new(),
        });

        assert!(payload.contains("\"event\":\"pre_tool_use\""));
        assert!(payload.contains("\"tool_name\":\"read_file\""));
        assert!(payload.contains("\"path\":\"src/main.rs\""));
        assert!(payload.contains("\"metadata\":{}"));
    }

    #[test]
    fn hook_payload_includes_metadata() {
        let payload = hook_payload(HookPayload {
            event: HookEvent::PermissionRequest,
            task: "inspect",
            tool_name: Some("run_shell"),
            tool_input: Some(&ToolInput::new().with_arg("command", "cargo test")),
            tool_status: None,
            tool_output: None,
            metadata: BTreeMap::from([
                ("permission_kind".to_string(), "shell".to_string()),
                ("permission_target".to_string(), "cargo test".to_string()),
            ]),
        });

        assert!(payload.contains("\"event\":\"permission_request\""));
        assert!(payload.contains("\"permission_kind\":\"shell\""));
        assert!(payload.contains("\"permission_target\":\"cargo test\""));
    }

    #[test]
    fn hook_stdout_decision_supports_json_allow_context() {
        let decision = hook_stdout_decision(
            HookEvent::SessionStart,
            r#"{"decision":"allow","add_context":"load me"}"#,
        )
        .unwrap();

        match decision {
            HookStdoutDecision::Allow(Some(context)) => assert_eq!(context, "load me"),
            _ => panic!("expected allow with context"),
        }
    }

    #[test]
    fn hook_stdout_decision_supports_json_deny() {
        let decision = hook_stdout_decision(
            HookEvent::PreToolUse,
            r#"{"decision":"deny","reason":"no"}"#,
        )
        .unwrap();

        match decision {
            HookStdoutDecision::Deny(reason) => assert_eq!(reason, "no"),
            _ => panic!("expected deny"),
        }
    }

    #[test]
    #[cfg(unix)]
    fn user_prompt_submit_collects_hook_stdout() {
        let root = temp_root("user-prompt");
        let hook = root.join("hooks/user_prompt_submit/10-context");
        write_hook(&hook, "printf 'extra context'");
        let runner = HookRunner::new(&HooksConfig {
            enabled: true,
            project_dir: root.join("hooks").display().to_string(),
            ..HooksConfig::default()
        });

        let output = runner.user_prompt_submit("task").unwrap().unwrap();
        assert!(output.contains("extra context"));
    }

    #[test]
    #[cfg(unix)]
    fn hook_runner_runs_user_hooks_before_project_hooks() {
        let root = temp_root("order");
        let user_hook = root.join("user/user_prompt_submit/10-user");
        let project_hook = root.join("project/user_prompt_submit/10-project");
        write_hook(&user_hook, "printf 'user hook'");
        write_hook(&project_hook, "printf 'project hook'");
        let runner = HookRunner::new(&HooksConfig {
            enabled: true,
            user_dir: root.join("user").display().to_string(),
            project_dir: root.join("project").display().to_string(),
            ..HooksConfig::default()
        });

        let output = runner.user_prompt_submit("task").unwrap().unwrap();

        let user_index = output.find("user hook").unwrap();
        let project_index = output.find("project hook").unwrap();
        assert!(user_index < project_index);
    }

    #[test]
    #[cfg(unix)]
    fn pre_tool_use_nonzero_blocks() {
        let root = temp_root("pre-block");
        let hook = root.join("hooks/pre_tool_use/10-block");
        write_hook(&hook, "printf 'blocked by test' >&2\nexit 2");
        let runner = HookRunner::new(&HooksConfig {
            enabled: true,
            project_dir: root.join("hooks").display().to_string(),
            ..HooksConfig::default()
        });

        let error = runner
            .pre_tool_use("task", "read_file", &ToolInput::new())
            .unwrap_err();

        assert_eq!(
            crate::error::classify(error.as_ref()),
            crate::error::AppErrorKind::PolicyDenied,
        );
        assert!(error.to_string().contains("blocked by test"));
    }

    #[test]
    #[cfg(unix)]
    fn post_tool_use_nonzero_is_advisory_context() {
        let root = temp_root("post-fail");
        let hook = root.join("hooks/post_tool_use/10-note");
        write_hook(&hook, "printf 'post failed note' >&2\nexit 3");
        let runner = HookRunner::new(&HooksConfig {
            enabled: true,
            project_dir: root.join("hooks").display().to_string(),
            ..HooksConfig::default()
        });

        let output = runner
            .post_tool_use(
                "task",
                "read_file",
                &BTreeMap::new(),
                crate::model::protocol::ObservationStatus::Ok,
                "output",
            )
            .unwrap()
            .unwrap();

        assert!(output.contains("post failed note"));
    }

    #[test]
    #[cfg(unix)]
    fn pre_compact_collects_hook_stdout() {
        let root = temp_root("pre-compact");
        let hook = root.join("hooks/pre_compact/10-note");
        write_hook(&hook, "printf 'compact note'");
        let runner = HookRunner::new(&HooksConfig {
            enabled: true,
            project_dir: root.join("hooks").display().to_string(),
            ..HooksConfig::default()
        });

        let output = runner
            .pre_compact("task", "manual_repl_compact")
            .unwrap()
            .unwrap();

        assert!(output.contains("compact note"));
    }
}
