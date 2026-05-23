use std::cell::RefCell;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::cli::app::HooksAction;
use crate::config::load::load_or_default;
use crate::config::types::AppConfig;
use crate::core::context::TaskContext;
use crate::core::loop_runtime::{AgentLoop, AgentLoopOptions};
use crate::error::{app_error, AppResult};
use crate::model::client::ModelClient;
use crate::model::protocol::{
    ModelAction, ModelRequest, ModelResponse, ObservationStatus, TokenUsage,
};
use crate::tools::types::ToolInput;
use crate::ui::stream::StreamEvents;
use crate::util::json::{json_as_string, json_escape, parse_root_object};

#[cfg(unix)]
const HOOK_FIXTURE_EVENTS: [&str; 5] = [
    "session_start",
    "user_prompt_submit",
    "pre_tool_use",
    "post_tool_use",
    "session_stop",
];

pub fn run(action: HooksAction) -> AppResult<()> {
    match action {
        HooksAction::FixtureSmoke { json } => fixture_smoke(json),
    }
}

#[derive(Debug, Clone)]
struct HooksFixtureSmokeReport {
    workdir: PathBuf,
    event_order: Vec<String>,
    session_start_ok: bool,
    user_prompt_submit_ok: bool,
    pre_tool_ok: bool,
    post_tool_ok: bool,
    session_stop_ok: bool,
    hook_contexts_ok: bool,
    tool_ran_ok: bool,
    tool_events: usize,
    final_message: String,
}

#[derive(Debug, Clone)]
struct RecordedHookEvent {
    event: String,
    payload_event: String,
    tool_name: Option<String>,
    tool_status: Option<String>,
}

struct HookFixtureClient {
    workspace: PathBuf,
    calls: RefCell<usize>,
    hook_observations: RefCell<BTreeSet<String>>,
}

impl HookFixtureClient {
    fn new(workspace: PathBuf) -> Self {
        Self {
            workspace,
            calls: RefCell::new(0),
            hook_observations: RefCell::new(BTreeSet::new()),
        }
    }
}

impl ModelClient for HookFixtureClient {
    fn respond(
        &self,
        input: ModelRequest,
        _events: &mut dyn StreamEvents,
    ) -> AppResult<(ModelResponse, Option<TokenUsage>)> {
        for observation in input
            .observations
            .iter()
            .filter(|observation| observation.tool_name == "hook")
        {
            self.hook_observations
                .borrow_mut()
                .insert(observation.summary.clone());
        }

        let call_index = *self.calls.borrow();
        *self.calls.borrow_mut() = call_index + 1;
        let action = if call_index == 0 {
            ModelAction::CallTool {
                tool_name: "list_files".to_string(),
                input: ToolInput::new()
                    .with_arg("root", self.workspace.display().to_string())
                    .with_arg("max_depth", "1"),
            }
        } else {
            ModelAction::Finish
        };
        let message = if call_index == 0 {
            "hooks fixture listing workspace"
        } else {
            "hooks fixture complete"
        };
        Ok((
            ModelResponse {
                message: message.to_string(),
                action,
            },
            None,
        ))
    }
}

fn fixture_smoke(json: bool) -> AppResult<()> {
    let base_config = load_or_default()?;
    let root = hooks_fixture_temp_root()?;
    fs::create_dir_all(&root)?;
    let result = run_hooks_fixture_smoke_at(&base_config, &root);
    if result.is_ok() {
        fs::remove_dir_all(&root).map_err(|error| {
            app_error(format!(
                "hooks fixture smoke passed, but failed to remove {}: {error}",
                root.display()
            ))
        })?;
    }
    let report = result?;
    if json {
        println!("{}", render_hooks_fixture_smoke_json(&report));
    } else {
        print_hooks_fixture_smoke_report(&report);
    }
    Ok(())
}

fn run_hooks_fixture_smoke_at(
    base_config: &AppConfig,
    root: &Path,
) -> AppResult<HooksFixtureSmokeReport> {
    let workspace = root.join("workspace");
    fs::create_dir_all(&workspace)?;
    fs::write(workspace.join("README.md"), "DeepSeekCode hooks fixture\n")?;

    let hooks_root = root.join("hooks");
    let log_path = root.join("hook-events.log");
    install_hook_fixture_scripts(&hooks_root, &log_path)?;

    let mut config = base_config.clone();
    config.hooks.enabled = true;
    config.hooks.project_dir = hooks_root.display().to_string();
    config.hooks.user_dir = root.join("missing-user-hooks").display().to_string();
    config.hooks.timeout_ms = 5_000;
    config.workspace.config_dir = root.join(".dscode").display().to_string();
    config.workspace.session_dir = root.join(".dscode/sessions").display().to_string();
    config.workspace.user_skills_dir = root.join("missing-user-skills").display().to_string();
    config.workspace.user_commands_dir = root.join("missing-user-commands").display().to_string();
    config.workspace.user_instructions_file =
        root.join("missing-user-AGENTS.md").display().to_string();
    config.memory.enabled = false;
    config.mcp.enabled = false;
    config.network.audit = false;

    let client = HookFixtureClient::new(workspace);
    let result = AgentLoop::new(config).run_with_client(
        TaskContext::new("hooks fixture smoke".to_string(), None),
        AgentLoopOptions {
            steps: 2,
            emit_progress: false,
            persist_session: false,
            ..AgentLoopOptions::default()
        },
        &client,
    )?;

    let events = parse_hook_event_log(&log_path)?;
    let hook_observations = client
        .hook_observations
        .borrow()
        .iter()
        .cloned()
        .collect::<Vec<_>>();
    let report = HooksFixtureSmokeReport {
        workdir: root.to_path_buf(),
        event_order: events.iter().map(|event| event.event.clone()).collect(),
        session_start_ok: event_seen(&events, "session_start", None, None),
        user_prompt_submit_ok: event_seen(&events, "user_prompt_submit", None, None),
        pre_tool_ok: event_seen(&events, "pre_tool_use", Some("list_files"), None),
        post_tool_ok: event_seen(&events, "post_tool_use", Some("list_files"), Some("ok")),
        session_stop_ok: event_seen(&events, "session_stop", None, None),
        hook_contexts_ok: [
            "session_start",
            "user_prompt_submit",
            "pre_tool_use",
            "post_tool_use",
        ]
        .iter()
        .all(|needle| {
            hook_observations
                .iter()
                .any(|summary| summary.contains(needle))
        }),
        tool_ran_ok: result.tool_events.iter().any(|event| {
            event.tool_name == "list_files"
                && event.status == ObservationStatus::Ok
                && event.output.contains("README.md")
        }),
        tool_events: result.tool_events.len(),
        final_message: result.final_message,
    };

    if !report.all_ok() {
        return Err(app_error(format!(
            "hooks fixture smoke failed: {}",
            render_hooks_fixture_smoke_json(&report)
        )));
    }

    Ok(report)
}

impl HooksFixtureSmokeReport {
    fn all_ok(&self) -> bool {
        self.session_start_ok
            && self.user_prompt_submit_ok
            && self.pre_tool_ok
            && self.post_tool_ok
            && self.session_stop_ok
            && self.hook_contexts_ok
            && self.tool_ran_ok
    }
}

#[cfg(unix)]
fn install_hook_fixture_scripts(hooks_root: &Path, log_path: &Path) -> AppResult<()> {
    use std::os::unix::fs::PermissionsExt;

    let log_path = shell_double_quoted(&log_path.display().to_string());
    let script = format!(
        "#!/bin/sh\npayload=$(cat)\nprintf '%s\\t%s\\n' \"$DSCODE_HOOK_EVENT\" \"$payload\" >> \"{log_path}\"\nprintf '{{\"decision\":\"allow\",\"add_context\":\"%s context\"}}\\n' \"$DSCODE_HOOK_EVENT\"\n"
    );
    for event in HOOK_FIXTURE_EVENTS {
        let dir = hooks_root.join(event);
        fs::create_dir_all(&dir)?;
        let path = dir.join("10-record");
        fs::write(&path, &script)?;
        let mut permissions = fs::metadata(&path)?.permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(path, permissions)?;
    }
    Ok(())
}

#[cfg(not(unix))]
fn install_hook_fixture_scripts(_hooks_root: &Path, _log_path: &Path) -> AppResult<()> {
    Err(app_error(
        "hooks fixture-smoke currently requires a Unix-like shell",
    ))
}

#[cfg(unix)]
fn shell_double_quoted(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('$', "\\$")
        .replace('`', "\\`")
}

fn parse_hook_event_log(path: &Path) -> AppResult<Vec<RecordedHookEvent>> {
    let content = fs::read_to_string(path)
        .map_err(|error| app_error(format!("failed to read hook fixture log: {error}")))?;
    let mut events = Vec::new();
    for (line_index, line) in content.lines().enumerate() {
        let Some((event, payload)) = line.split_once('\t') else {
            return Err(app_error(format!(
                "hook fixture log line {} is malformed",
                line_index + 1
            )));
        };
        let root = parse_root_object(payload)?;
        let payload_event = root
            .get("event")
            .and_then(json_as_string)
            .ok_or_else(|| app_error("hook payload missing string `event`"))?;
        let tool_name = root
            .get("tool_name")
            .and_then(json_as_string)
            .map(str::to_string);
        let tool_status = root
            .get("tool_status")
            .and_then(json_as_string)
            .map(str::to_string);
        events.push(RecordedHookEvent {
            event: event.to_string(),
            payload_event: payload_event.to_string(),
            tool_name,
            tool_status,
        });
    }
    Ok(events)
}

fn event_seen(
    events: &[RecordedHookEvent],
    expected: &str,
    tool_name: Option<&str>,
    tool_status: Option<&str>,
) -> bool {
    events.iter().any(|event| {
        event.event == expected
            && event.payload_event == expected
            && tool_name
                .map(|expected_tool| event.tool_name.as_deref() == Some(expected_tool))
                .unwrap_or(true)
            && tool_status
                .map(|expected_status| event.tool_status.as_deref() == Some(expected_status))
                .unwrap_or(true)
    })
}

fn hooks_fixture_temp_root() -> AppResult<PathBuf> {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| app_error(format!("system clock error: {error}")))?
        .as_nanos();
    Ok(std::env::temp_dir().join(format!(
        "deepseek-hooks-fixture-{}-{nanos}",
        std::process::id()
    )))
}

fn render_hooks_fixture_smoke_json(report: &HooksFixtureSmokeReport) -> String {
    let events = report
        .event_order
        .iter()
        .map(|event| format!("\"{}\"", json_escape(event)))
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "{{\"kind\":\"deepseek.hooks_fixture_smoke.v1\",\"workdir\":\"{}\",\"session_start_ok\":{},\"user_prompt_submit_ok\":{},\"pre_tool_ok\":{},\"post_tool_ok\":{},\"session_stop_ok\":{},\"hook_contexts_ok\":{},\"tool_ran_ok\":{},\"tool_events\":{},\"final_message\":\"{}\",\"events\":[{}]}}",
        json_escape(&report.workdir.display().to_string()),
        report.session_start_ok,
        report.user_prompt_submit_ok,
        report.pre_tool_ok,
        report.post_tool_ok,
        report.session_stop_ok,
        report.hook_contexts_ok,
        report.tool_ran_ok,
        report.tool_events,
        json_escape(&report.final_message),
        events
    )
}

fn print_hooks_fixture_smoke_report(report: &HooksFixtureSmokeReport) {
    println!("Hooks fixture smoke: ok");
    println!("workdir: {}", report.workdir.display());
    println!("events: {}", report.event_order.join(" -> "));
    println!(
        "session_start={} user_prompt_submit={} pre_tool={} post_tool={} session_stop={}",
        report.session_start_ok,
        report.user_prompt_submit_ok,
        report.pre_tool_ok,
        report.post_tool_ok,
        report.session_stop_ok
    );
    println!(
        "hook_contexts={} tool_ran={} tool_events={}",
        report.hook_contexts_ok, report.tool_ran_ok, report.tool_events
    );
    println!("final_message: {}", report.final_message);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    fn temp_root(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "deepseek_hooks_command_test_{name}_{}_{nanos}",
            std::process::id()
        ))
    }

    #[test]
    fn fixture_smoke_json_reports_hook_lifecycle() {
        let report = HooksFixtureSmokeReport {
            workdir: PathBuf::from("/tmp/deepseek-hooks-fixture"),
            event_order: vec![
                "session_start".to_string(),
                "user_prompt_submit".to_string(),
                "pre_tool_use".to_string(),
                "post_tool_use".to_string(),
                "session_stop".to_string(),
            ],
            session_start_ok: true,
            user_prompt_submit_ok: true,
            pre_tool_ok: true,
            post_tool_ok: true,
            session_stop_ok: true,
            hook_contexts_ok: true,
            tool_ran_ok: true,
            tool_events: 1,
            final_message: "hooks fixture complete".to_string(),
        };

        let rendered = render_hooks_fixture_smoke_json(&report);
        assert!(rendered.contains("\"kind\":\"deepseek.hooks_fixture_smoke.v1\""));
        assert!(rendered.contains("\"session_start_ok\":true"));
        assert!(rendered.contains("\"user_prompt_submit_ok\":true"));
        assert!(rendered.contains("\"pre_tool_ok\":true"));
        assert!(rendered.contains("\"post_tool_ok\":true"));
        assert!(rendered.contains("\"session_stop_ok\":true"));
        assert!(rendered.contains("\"hook_contexts_ok\":true"));
        assert!(rendered.contains("\"tool_ran_ok\":true"));
    }

    #[test]
    #[cfg(unix)]
    fn fixture_smoke_runs_agent_loop_hooks() {
        let root = temp_root("agent-loop");
        fs::create_dir_all(&root).unwrap();
        let report = run_hooks_fixture_smoke_at(&AppConfig::default(), &root).unwrap();

        assert!(report.session_start_ok);
        assert!(report.user_prompt_submit_ok);
        assert!(report.pre_tool_ok);
        assert!(report.post_tool_ok);
        assert!(report.session_stop_ok);
        assert!(report.hook_contexts_ok);
        assert!(report.tool_ran_ok);
        assert_eq!(report.final_message, "hooks fixture complete");

        let _ = fs::remove_dir_all(root);
    }
}
