use std::cell::RefCell;
use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Read, Write};
#[cfg(unix)]
use std::net::Shutdown;
use std::path::Path;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::rc::Rc;
#[cfg(unix)]
use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(unix)]
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode};

use crate::cli::app::{
    AgentsAction, AgentsRlmCancelArgs, AgentsRlmDrainArgs, AgentsRlmEventsArgs,
    AgentsRlmRecoverArgs, AgentsRlmRunNextArgs, AgentsRlmStatusArgs, AgentsRlmStopArgs,
    AgentsRlmWaitArgs, AgentsServiceArgs, AgentsServiceDoctorArgs, AgentsServiceKind,
    AgentsServiceSmokeArgs, AgentsShellAction, AgentsShellArgs, AgentsShellSupervisorArgs,
};
use crate::config::load::load_or_default;
use crate::config::types::AppConfig;
use crate::core::agents::{load_agent_file, load_default_agents, AgentLoadResult, AgentSource};
use crate::core::context::TaskContext;
use crate::core::loop_runtime::{
    AgentApprovalDecision, AgentApprovalRequest, AgentApprovalResolver, AgentLoop,
    AgentLoopOptions, AgentSessionBudget, AgentUserInputRequest, AgentUserInputResolver,
    AgentUserInputResponse, RunResult, SharedAgentApprovalResolver, SharedAgentUserInputResolver,
    ToolEvent,
};
use crate::core::prompt_layers::prompt_layers_event_payload;
use crate::core::rollback::RollbackStore;
use crate::core::runtime::{
    AutomationRecord, RuntimeStore, TaskRecord, ThreadCompactionRecord, ThreadRecord, TurnRecord,
};
use crate::error::{app_error, AppResult};
use crate::model::client::ModelClient;
use crate::model::deepseek::DeepSeekClient;
use crate::model::protocol::{ModelAction, ModelRequest, ObservationStatus};
use crate::tools::dispatch_subagent::{
    active_agent_thread_path, agent_threads_dir, thread_file_path, validate_thread_id,
};
#[cfg(all(unix, target_os = "linux"))]
use crate::tools::exec_shell::lease_native_supervisor_pty_master_fd;
use crate::tools::exec_shell::{
    count_active_durable_shell_jobs, native_supervisor_pty_supported, ExecShellAttachTool,
    ExecShellCancelTool, ExecShellInteractTool, ExecShellListTool, ExecShellReplayTool,
    ExecShellResizeTool, ExecShellSupervisorStatusTool, ExecShellWaitTool, TaskShellStartTool,
    SHELL_SUPERVISOR_SUPPORTED_METHODS, SHELL_SUPERVISOR_UNSUPPORTED_PTY_METHODS,
};
use crate::tools::rlm::{
    rlm_live_session_ids_by_runtime_thread, RlmLiveCancelTool, RlmLiveDrainTool, RlmLiveEventsTool,
    RlmLiveRecoverTool, RlmLiveRunNextTool, RlmLiveStatusTool, RlmLiveStopTool, RlmLiveWaitTool,
};
use crate::tools::types::{Tool, ToolInput};
use crate::util::json::{
    json_as_array, json_as_object, json_as_string, json_as_u64, parse_json_value,
};
use crate::util::json::{json_escape, json_value_to_string, JsonValue};

pub fn run(action: AgentsAction) -> AppResult<()> {
    let config = load_or_default()?;
    match action {
        AgentsAction::List => list_agents(&config.workspace.config_dir),
        AgentsAction::Show { name } => show_agent(&config.workspace.config_dir, &name),
        AgentsAction::Validate { path } => validate_agents(&config.workspace.config_dir, path),
        AgentsAction::RunTask { id, budget, json } => run_runtime_task(config, &id, budget, json),
        AgentsAction::Daemon {
            budget,
            interval_ms,
            once,
            json,
        } => run_runtime_daemon(config, budget, interval_ms, once, json),
        AgentsAction::RlmStatus(args) => run_rlm_status(config, args),
        AgentsAction::RlmEvents(args) => run_rlm_events(config, args),
        AgentsAction::RlmWait(args) => run_rlm_wait(config, args),
        AgentsAction::RlmCancel(args) => run_rlm_cancel(config, args),
        AgentsAction::RlmRecover(args) => run_rlm_recover(config, args),
        AgentsAction::RlmStop(args) => run_rlm_stop(config, args),
        AgentsAction::RlmRunNext(args) => run_rlm_run_next(config, args),
        AgentsAction::RlmDrain(args) => run_rlm_drain(config, args),
        AgentsAction::Shell(args) => run_shell_control(args),
        AgentsAction::ShellSupervisor(args) => run_shell_supervisor(args),
        AgentsAction::Service(args) => render_agent_services(args),
        AgentsAction::ServiceDoctor(args) => run_service_doctor(args),
        AgentsAction::ServiceSmoke(args) => run_service_smoke(args),
        AgentsAction::ShellFixtureSmoke { json } => run_shell_fixture_smoke(json),
        AgentsAction::SubagentFixtureSmoke { json } => run_subagent_fixture_smoke(json),
        AgentsAction::Threads => list_threads(&config.workspace.config_dir),
        AgentsAction::ShowThread { id } => show_thread(&config.workspace.config_dir, &id),
        AgentsAction::SwitchThread { id } => switch_thread(&config.workspace.config_dir, &id),
        AgentsAction::CurrentThread => current_thread(&config.workspace.config_dir),
        AgentsAction::ClearThread => clear_thread(&config.workspace.config_dir),
    }
}

fn run_shell_control(args: AgentsShellArgs) -> AppResult<()> {
    if agents_shell_fd_proxy_requested(&args) {
        return run_shell_fd_proxy(args);
    }
    if agents_shell_byte_stream_requested(&args) {
        return run_shell_byte_stream(args);
    }
    if agents_shell_attach_interactive_requested(&args) {
        return run_shell_attach_interactive(args);
    }
    if agents_shell_attach_follow_requested(&args) || agents_shell_attach_raw_requested(&args) {
        return run_shell_attach_follow(args);
    }
    let cwd = std::env::current_dir()?;
    let request = agents_shell_request_json(&args);
    let response = send_shell_supervisor_cli_request(&cwd, &request)?;
    print_shell_control_response(&args, response)
}

fn agents_shell_attach_interactive_requested(args: &AgentsShellArgs) -> bool {
    matches!(
        &args.action,
        AgentsShellAction::Attach {
            interactive: true,
            ..
        }
    )
}

fn agents_shell_attach_follow_requested(args: &AgentsShellArgs) -> bool {
    matches!(&args.action, AgentsShellAction::Attach { follow: true, .. })
}

fn agents_shell_attach_raw_requested(args: &AgentsShellArgs) -> bool {
    matches!(&args.action, AgentsShellAction::Attach { raw: true, .. })
}

fn agents_shell_byte_stream_requested(args: &AgentsShellArgs) -> bool {
    matches!(&args.action, AgentsShellAction::ByteStream { .. })
}

fn agents_shell_fd_proxy_requested(args: &AgentsShellArgs) -> bool {
    matches!(&args.action, AgentsShellAction::FdProxy { .. })
}

fn run_shell_attach_interactive(args: AgentsShellArgs) -> AppResult<()> {
    if args.json {
        return Err(app_error(
            "agents shell attach --interactive cannot be combined with --json",
        ));
    }
    let AgentsShellAction::Attach {
        task_id,
        cursor,
        wait_ms: _,
        limit_bytes,
        tail,
        follow: _,
        interactive: _,
        raw,
        poll_ms,
        max_ms,
    } = args.action
    else {
        return Err(app_error(
            "agents shell attach --interactive requires attach action",
        ));
    };
    if raw {
        return Err(app_error(
            "agents shell attach --interactive cannot be combined with --raw",
        ));
    }
    let cwd = std::env::current_dir()?;
    let limit_bytes = limit_bytes.unwrap_or(16 * 1024);
    let poll_delay = Duration::from_millis(poll_ms.unwrap_or(50).clamp(10, 1000));
    let max_duration = max_ms.map(Duration::from_millis);
    let started = Instant::now();
    let mut stdout_offset = cursor.unwrap_or(0);
    let mut last_output_poll = Instant::now() - poll_delay;
    eprintln!("attached to {task_id}; detach with Ctrl-]");
    let _raw = ShellRawModeGuard::enter()?;

    let _ = shell_attach_interactive_resize_to_terminal(&cwd, &task_id);
    #[cfg(debug_assertions)]
    {
        // Debug-only hook for the PTY integration smoke; no user-facing flag depends on it.
        if let Ok(input) = std::env::var("DSCODE_TEST_AGENTS_SHELL_ATTACH_INTERACTIVE_INPUT") {
            if !input.is_empty() {
                shell_attach_interactive_send_stdin(&cwd, &task_id, &input)?;
            }
        }
    }
    loop {
        while event::poll(Duration::from_millis(1))? {
            match event::read()? {
                Event::Key(key) if shell_attach_interactive_is_detach_key(key) => {
                    return Ok(());
                }
                Event::Key(key) => {
                    if let Some(input) = shell_attach_interactive_key_input(key) {
                        shell_attach_interactive_send_stdin(&cwd, &task_id, &input)?;
                    }
                }
                Event::Resize(cols, rows) => {
                    let _ = shell_attach_interactive_send_resize(&cwd, &task_id, rows, cols);
                }
                Event::Paste(input) => {
                    if !input.is_empty() {
                        shell_attach_interactive_send_stdin(&cwd, &task_id, &input)?;
                    }
                }
                _ => {}
            }
        }

        if last_output_poll.elapsed() >= poll_delay {
            let (next_offset, status, advanced) = shell_attach_interactive_poll_stdout(
                &cwd,
                &task_id,
                stdout_offset,
                limit_bytes,
                tail && stdout_offset == 0,
            )?;
            stdout_offset = next_offset;
            last_output_poll = Instant::now();
            if status != "running" && !advanced {
                break;
            }
        }
        if max_duration.is_some_and(|max| started.elapsed() >= max) {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    Ok(())
}

struct ShellRawModeGuard;

impl ShellRawModeGuard {
    fn enter() -> AppResult<Self> {
        enable_raw_mode()?;
        Ok(Self)
    }
}

impl Drop for ShellRawModeGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
    }
}

fn shell_attach_interactive_poll_stdout(
    cwd: &Path,
    task_id: &str,
    offset: u64,
    limit_bytes: u64,
    tail: bool,
) -> AppResult<(u64, String, bool)> {
    let request_args = AgentsShellArgs {
        action: AgentsShellAction::Attach {
            task_id: task_id.to_string(),
            cursor: Some(offset),
            wait_ms: Some(0),
            limit_bytes: Some(limit_bytes),
            tail,
            follow: false,
            interactive: false,
            raw: false,
            poll_ms: None,
            max_ms: None,
        },
        json: false,
    };
    let response =
        send_shell_supervisor_cli_request(cwd, &agents_shell_request_json(&request_args))?;
    let object = json_as_object(&response)
        .ok_or_else(|| app_error("shell supervisor response must be a JSON object"))?;
    let response_status = object
        .get("status")
        .and_then(json_as_string)
        .unwrap_or("unknown");
    if response_status == "error" || response_status == "unsupported" {
        let error = object
            .get("error")
            .and_then(json_as_string)
            .unwrap_or("shell supervisor interactive attach failed");
        return Err(app_error(error.to_string()));
    }
    let summary = object
        .get("attach_summary")
        .and_then(json_as_string)
        .ok_or_else(|| app_error("shell supervisor attach response missing attach_summary"))?;
    let mut stdout = std::io::stdout();
    let wrote =
        shell_attach_write_response_terminal_payload(object, summary, &mut stdout, false, false)?;
    let next_offset = shell_attach_summary_next_cursor(summary).unwrap_or(offset);
    let status = shell_summary_value(summary, "status")
        .unwrap_or("unknown")
        .to_string();
    Ok((next_offset, status, wrote || next_offset > offset))
}

fn shell_attach_interactive_send_stdin(cwd: &Path, task_id: &str, input: &str) -> AppResult<()> {
    let request_args = AgentsShellArgs {
        action: AgentsShellAction::Stdin {
            task_id: task_id.to_string(),
            input: Some(input.to_string()),
            close_stdin: false,
            timeout_ms: Some(500),
        },
        json: false,
    };
    let response =
        send_shell_supervisor_cli_request(cwd, &agents_shell_request_json(&request_args))?;
    shell_attach_interactive_check_control_response(&response, "stdin")
}

fn shell_attach_interactive_send_resize(
    cwd: &Path,
    task_id: &str,
    rows: u16,
    cols: u16,
) -> AppResult<()> {
    let request_args = AgentsShellArgs {
        action: AgentsShellAction::Resize {
            task_id: task_id.to_string(),
            tty_rows: u64::from(rows),
            tty_cols: u64::from(cols),
        },
        json: false,
    };
    let response =
        send_shell_supervisor_cli_request(cwd, &agents_shell_request_json(&request_args))?;
    shell_attach_interactive_check_control_response(&response, "resize")
}

fn shell_attach_interactive_resize_to_terminal(cwd: &Path, task_id: &str) -> AppResult<()> {
    let (cols, rows) = crossterm::terminal::size()?;
    shell_attach_interactive_send_resize(cwd, task_id, rows, cols)
}

fn shell_attach_interactive_check_control_response(
    response: &JsonValue,
    action: &str,
) -> AppResult<()> {
    let object = json_as_object(response)
        .ok_or_else(|| app_error("shell supervisor response must be a JSON object"))?;
    let status = object
        .get("status")
        .and_then(json_as_string)
        .unwrap_or("unknown");
    if status == "error" || status == "unsupported" {
        let error = object
            .get("error")
            .and_then(json_as_string)
            .unwrap_or("shell supervisor control request failed");
        return Err(app_error(format!(
            "interactive attach {action} failed: {error}"
        )));
    }
    Ok(())
}

fn shell_attach_interactive_is_detach_key(key: KeyEvent) -> bool {
    key.kind != KeyEventKind::Release
        && key.modifiers.contains(KeyModifiers::CONTROL)
        && matches!(key.code, KeyCode::Char(']'))
}

fn shell_attach_interactive_key_input(key: KeyEvent) -> Option<String> {
    if key.kind == KeyEventKind::Release {
        return None;
    }
    if key.modifiers.contains(KeyModifiers::CONTROL) {
        return shell_attach_interactive_control_key_input(key.code);
    }
    let prefix = if key.modifiers.contains(KeyModifiers::ALT) {
        "\x1b"
    } else {
        ""
    };
    match key.code {
        KeyCode::Backspace => Some("\x7f".to_string()),
        KeyCode::Enter => Some("\r".to_string()),
        KeyCode::Left => Some(format!("{prefix}\x1b[D")),
        KeyCode::Right => Some(format!("{prefix}\x1b[C")),
        KeyCode::Up => Some(format!("{prefix}\x1b[A")),
        KeyCode::Down => Some(format!("{prefix}\x1b[B")),
        KeyCode::Home => Some(format!("{prefix}\x1b[H")),
        KeyCode::End => Some(format!("{prefix}\x1b[F")),
        KeyCode::PageUp => Some(format!("{prefix}\x1b[5~")),
        KeyCode::PageDown => Some(format!("{prefix}\x1b[6~")),
        KeyCode::Tab => Some(format!("{prefix}\t")),
        KeyCode::BackTab => Some(format!("{prefix}\x1b[Z")),
        KeyCode::Delete => Some(format!("{prefix}\x1b[3~")),
        KeyCode::Insert => Some(format!("{prefix}\x1b[2~")),
        KeyCode::Esc => Some("\x1b".to_string()),
        KeyCode::Char(ch) => Some(format!("{prefix}{ch}")),
        _ => None,
    }
}

fn shell_attach_interactive_control_key_input(code: KeyCode) -> Option<String> {
    match code {
        KeyCode::Char(ch) => {
            let upper = ch.to_ascii_uppercase();
            if upper.is_ascii_uppercase() {
                let byte = (upper as u8).saturating_sub(b'A').saturating_add(1);
                return Some((byte as char).to_string());
            }
            match ch {
                '[' => Some("\x1b".to_string()),
                '\\' => Some("\x1c".to_string()),
                '^' => Some("\x1e".to_string()),
                '_' => Some("\x1f".to_string()),
                ' ' => Some("\0".to_string()),
                _ => None,
            }
        }
        _ => shell_attach_interactive_key_input(KeyEvent::new(code, KeyModifiers::NONE)),
    }
}

fn run_shell_attach_follow(args: AgentsShellArgs) -> AppResult<()> {
    let AgentsShellAction::Attach {
        task_id,
        cursor,
        wait_ms,
        limit_bytes,
        tail,
        follow,
        interactive: _,
        raw,
        poll_ms,
        max_ms,
    } = args.action
    else {
        return Err(app_error(
            "agents shell attach --follow requires attach action",
        ));
    };
    if args.json && raw {
        return Err(app_error(
            "agents shell attach --raw cannot be combined with --json",
        ));
    }
    let cwd = std::env::current_dir()?;
    if follow {
        return run_shell_attach_stream_follow(ShellAttachStreamFollow {
            cwd: &cwd,
            task_id: &task_id,
            cursor,
            wait_ms,
            limit_bytes,
            tail,
            raw,
            poll_ms,
            max_ms,
            json: args.json,
        });
    }
    let per_request_wait_ms = if follow {
        wait_ms.or(Some(1000))
    } else {
        wait_ms
    };
    let poll_delay = Duration::from_millis(poll_ms.unwrap_or(100).min(5000));
    let max_duration = max_ms.map(Duration::from_millis);
    let started = Instant::now();
    let mut current_cursor = cursor.unwrap_or(0);
    let mut first_request = true;

    loop {
        let request_args = AgentsShellArgs {
            action: AgentsShellAction::Attach {
                task_id: task_id.clone(),
                cursor: Some(current_cursor),
                wait_ms: per_request_wait_ms,
                limit_bytes,
                tail: tail && first_request,
                follow: false,
                interactive: false,
                raw: false,
                poll_ms: None,
                max_ms: None,
            },
            json: args.json,
        };
        let response =
            send_shell_supervisor_cli_request(&cwd, &agents_shell_request_json(&request_args))?;
        let object = json_as_object(&response)
            .ok_or_else(|| app_error("shell supervisor response must be a JSON object"))?;
        let response_status = object
            .get("status")
            .and_then(json_as_string)
            .unwrap_or("unknown");
        if response_status == "error" || response_status == "unsupported" {
            let error = object
                .get("error")
                .and_then(json_as_string)
                .unwrap_or("shell supervisor attach follow failed");
            return Err(app_error(error.to_string()));
        }
        if args.json {
            println!("{}", json_value_to_string(&response));
        }
        let summary = object
            .get("attach_summary")
            .and_then(json_as_string)
            .ok_or_else(|| app_error("shell supervisor attach response missing attach_summary"))?;
        if !args.json {
            print_shell_attach_follow_payload(object, summary, raw)?;
        }
        let next_cursor = shell_attach_summary_next_cursor(summary).unwrap_or(current_cursor);
        let advanced = next_cursor > current_cursor;
        current_cursor = next_cursor;
        first_request = false;
        if !follow {
            break;
        }
        let job_status = shell_summary_value(summary, "status").unwrap_or("unknown");
        if job_status != "running" {
            break;
        }
        if max_duration.is_some_and(|max| started.elapsed() >= max) {
            break;
        }
        if !advanced {
            std::thread::sleep(poll_delay);
        }
    }
    Ok(())
}

struct ShellAttachStreamFollow<'a> {
    cwd: &'a Path,
    task_id: &'a str,
    cursor: Option<u64>,
    wait_ms: Option<u64>,
    limit_bytes: Option<u64>,
    tail: bool,
    raw: bool,
    poll_ms: Option<u64>,
    max_ms: Option<u64>,
    json: bool,
}

fn run_shell_attach_stream_follow(args: ShellAttachStreamFollow<'_>) -> AppResult<()> {
    let request = shell_attach_stream_request_json(&args);
    let mut reader = open_shell_supervisor_cli_stream(args.cwd, &request)?;
    let mut line = String::new();
    loop {
        line.clear();
        let read = reader.read_line(&mut line)?;
        if read == 0 {
            break;
        }
        let value = parse_json_value(line.trim())?;
        let object = json_as_object(&value)
            .ok_or_else(|| app_error("shell supervisor stream response must be a JSON object"))?;
        let status = object
            .get("status")
            .and_then(json_as_string)
            .unwrap_or("unknown");
        if status == "error" || status == "unsupported" {
            let error = object
                .get("error")
                .and_then(json_as_string)
                .unwrap_or("shell supervisor attach stream failed");
            return Err(app_error(error.to_string()));
        }
        if args.json {
            println!("{}", json_value_to_string(&value));
        } else if let Some(summary) = object.get("attach_summary").and_then(json_as_string) {
            print_shell_attach_follow_payload(object, summary, args.raw)?;
        }
        if matches!(object.get("stream_done"), Some(JsonValue::Bool(true))) {
            break;
        }
    }
    Ok(())
}

fn run_shell_byte_stream(args: AgentsShellArgs) -> AppResult<()> {
    if agents_shell_byte_stream_raw_proxy_requested(&args) {
        if args.json {
            return Err(app_error(
                "agents shell byte-stream --raw-proxy cannot be combined with --json",
            ));
        }
        return run_shell_byte_stream_raw_proxy(args);
    }
    let cwd = std::env::current_dir()?;
    let request = agents_shell_request_json(&args);
    let mut reader = open_shell_supervisor_cli_stream(&cwd, &request)?;
    let mut line = String::new();
    loop {
        line.clear();
        let read = reader.read_line(&mut line)?;
        if read == 0 {
            break;
        }
        let value = parse_json_value(line.trim())?;
        let object = json_as_object(&value).ok_or_else(|| {
            app_error("shell supervisor byte stream response must be a JSON object")
        })?;
        let status = object
            .get("status")
            .and_then(json_as_string)
            .unwrap_or("unknown");
        if status == "error" || status == "unsupported" {
            let error = object
                .get("error")
                .and_then(json_as_string)
                .unwrap_or("shell supervisor byte stream failed");
            return Err(app_error(error.to_string()));
        }
        if args.json {
            println!("{}", json_value_to_string(&value));
        } else if let Some(summary) = object.get("attach_summary").and_then(json_as_string) {
            let mut stdout = std::io::stdout();
            let _ = shell_attach_write_response_terminal_payload(
                object,
                summary,
                &mut stdout,
                false,
                true,
            )?;
        }
        if matches!(object.get("stream_done"), Some(JsonValue::Bool(true))) {
            break;
        }
    }
    Ok(())
}

fn agents_shell_byte_stream_raw_proxy_requested(args: &AgentsShellArgs) -> bool {
    matches!(
        &args.action,
        AgentsShellAction::ByteStream {
            raw_proxy: true,
            ..
        }
    )
}

#[cfg(unix)]
fn agents_shell_byte_stream_terminal_proxy_requested(args: &AgentsShellArgs) -> bool {
    matches!(
        &args.action,
        AgentsShellAction::ByteStream {
            terminal_proxy: true,
            ..
        }
    )
}

#[cfg(unix)]
fn run_shell_byte_stream_raw_proxy(args: AgentsShellArgs) -> AppResult<()> {
    if agents_shell_byte_stream_terminal_proxy_requested(&args) {
        return run_shell_byte_stream_terminal_proxy(args);
    }
    let cwd = std::env::current_dir()?;
    let request = agents_shell_request_json(&args);
    let stream = open_shell_supervisor_cli_raw_stream(&cwd, &request)?;
    let mut reader = stream.try_clone()?;
    let mut writer = stream;
    std::thread::spawn(move || {
        let mut stdin = std::io::stdin().lock();
        let _ = std::io::copy(&mut stdin, &mut writer);
    });
    let mut stdout = std::io::stdout().lock();
    std::io::copy(&mut reader, &mut stdout)?;
    stdout.flush()?;
    Ok(())
}

#[cfg(unix)]
fn run_shell_byte_stream_terminal_proxy(args: AgentsShellArgs) -> AppResult<()> {
    let cwd = std::env::current_dir()?;
    let (task_id, proxy_args) = shell_byte_stream_terminal_proxy_args(args)?;
    let request = agents_shell_request_json(&proxy_args);
    let stream = open_shell_supervisor_cli_raw_stream(&cwd, &request)?;
    let mut reader = stream.try_clone()?;
    let writer = stream;
    eprintln!("proxied to {task_id}; detach with Ctrl-]");
    let _raw = ShellRawModeGuard::enter()?;
    let done = Arc::new(AtomicBool::new(false));

    #[cfg(debug_assertions)]
    if let Ok(input) = std::env::var("DSCODE_TEST_AGENTS_SHELL_PROXY_INPUT") {
        let mut writer = writer;
        if !input.is_empty() {
            writer.write_all(input.as_bytes())?;
            writer.flush()?;
        }
        let _ = writer.shutdown(Shutdown::Write);
        let mut stdout = std::io::stdout().lock();
        std::io::copy(&mut reader, &mut stdout)?;
        stdout.flush()?;
        done.store(true, Ordering::Relaxed);
        return Ok(());
    }

    let input_done = Arc::clone(&done);
    let input_cwd = cwd.clone();
    let input_task_id = task_id.clone();
    let input_handle = std::thread::spawn(move || {
        shell_byte_stream_terminal_proxy_input_loop(writer, input_cwd, input_task_id, input_done)
    });

    let mut stdout = std::io::stdout().lock();
    let copy_result = std::io::copy(&mut reader, &mut stdout);
    done.store(true, Ordering::Relaxed);
    let _ = input_handle.join();
    copy_result?;
    stdout.flush()?;
    Ok(())
}

#[cfg(unix)]
fn shell_byte_stream_terminal_proxy_args(
    args: AgentsShellArgs,
) -> AppResult<(String, AgentsShellArgs)> {
    let AgentsShellArgs { action, json } = args;
    let AgentsShellAction::ByteStream {
        task_id,
        cursor,
        wait_ms,
        limit_bytes,
        tail,
        input,
        close_stdin,
        mut tty_rows,
        mut tty_cols,
        poll_ms,
        max_ms,
        max_events,
        raw_proxy,
        terminal_proxy,
    } = action
    else {
        return Err(app_error("agents shell proxy requires byte-stream action"));
    };
    if tty_rows.is_none() && tty_cols.is_none() {
        if let Ok((cols, rows)) = crossterm::terminal::size() {
            tty_rows = Some(u64::from(rows));
            tty_cols = Some(u64::from(cols));
        }
    }
    let max_ms = max_ms.or(Some(300_000));
    let task_id_for_return = task_id.clone();
    Ok((
        task_id_for_return,
        AgentsShellArgs {
            action: AgentsShellAction::ByteStream {
                task_id,
                cursor,
                wait_ms,
                limit_bytes,
                tail,
                input,
                close_stdin,
                tty_rows,
                tty_cols,
                poll_ms,
                max_ms,
                max_events,
                raw_proxy,
                terminal_proxy,
            },
            json,
        },
    ))
}

#[cfg(unix)]
fn shell_byte_stream_terminal_proxy_input_loop(
    mut writer: std::os::unix::net::UnixStream,
    cwd: PathBuf,
    task_id: String,
    done: Arc<AtomicBool>,
) {
    while !done.load(Ordering::Relaxed) {
        match event::poll(Duration::from_millis(10)) {
            Ok(true) => match event::read() {
                Ok(Event::Key(key)) if shell_attach_interactive_is_detach_key(key) => {
                    done.store(true, Ordering::Relaxed);
                    let _ = writer.shutdown(Shutdown::Both);
                    break;
                }
                Ok(Event::Key(key)) => {
                    if let Some(input) = shell_attach_interactive_key_input(key) {
                        if writer.write_all(input.as_bytes()).is_err() {
                            done.store(true, Ordering::Relaxed);
                            break;
                        }
                        let _ = writer.flush();
                    }
                }
                Ok(Event::Resize(cols, rows)) => {
                    let _ = shell_attach_interactive_send_resize(&cwd, &task_id, rows, cols);
                }
                Ok(Event::Paste(input)) => {
                    if !input.is_empty() && writer.write_all(input.as_bytes()).is_err() {
                        done.store(true, Ordering::Relaxed);
                        break;
                    }
                    let _ = writer.flush();
                }
                Ok(_) => {}
                Err(_) => {
                    done.store(true, Ordering::Relaxed);
                    break;
                }
            },
            Ok(false) => {}
            Err(_) => {
                done.store(true, Ordering::Relaxed);
                break;
            }
        }
    }
}

#[cfg(not(unix))]
fn run_shell_byte_stream_raw_proxy(_args: AgentsShellArgs) -> AppResult<()> {
    Err(app_error(
        "agents shell byte-stream --raw-proxy currently requires the Unix shell supervisor socket",
    ))
}

#[cfg(all(unix, target_os = "linux"))]
fn run_shell_fd_proxy(args: AgentsShellArgs) -> AppResult<()> {
    if args.json {
        return Err(app_error(
            "agents shell fd-proxy cannot be combined with --json because it receives a PTY fd",
        ));
    }
    let AgentsShellAction::FdProxy {
        task_id,
        mut tty_rows,
        mut tty_cols,
        max_ms,
    } = args.action
    else {
        return Err(app_error("agents shell fd-proxy requires fd-proxy action"));
    };
    if tty_rows.is_none() && tty_cols.is_none() {
        if let Ok((cols, rows)) = crossterm::terminal::size() {
            tty_rows = Some(u64::from(rows));
            tty_cols = Some(u64::from(cols));
        }
    }
    let cwd = std::env::current_dir()?;
    let request = agents_shell_request_json(&AgentsShellArgs {
        action: AgentsShellAction::FdProxy {
            task_id: task_id.clone(),
            tty_rows,
            tty_cols,
            max_ms,
        },
        json: false,
    });
    let mut control = open_shell_supervisor_cli_raw_stream(&cwd, &request)?;
    let response_line = read_shell_supervisor_request_line(&mut control)?;
    let response = parse_json_value(response_line.trim())?;
    let object = json_as_object(&response)
        .ok_or_else(|| app_error("shell supervisor pty_fd response must be a JSON object"))?;
    if object
        .get("status")
        .and_then(json_as_string)
        .unwrap_or("unknown")
        != "ok"
    {
        let error = object
            .get("error")
            .and_then(json_as_string)
            .unwrap_or("shell supervisor pty_fd failed");
        return Err(app_error(error.to_string()));
    }
    let mut pty = receive_shell_supervisor_fd(&control)?;
    eprintln!("fd-proxied to {task_id}; detach with Ctrl-]");
    let _raw = ShellRawModeGuard::enter()?;
    shell_fd_proxy_set_nonblocking(&pty)?;

    #[cfg(debug_assertions)]
    if let Ok(input) = std::env::var("DSCODE_TEST_AGENTS_SHELL_FD_PROXY_INPUT") {
        if !input.is_empty() {
            pty.write_all(input.as_bytes())?;
            pty.flush()?;
        }
        let output = read_shell_fd_proxy_output_for(
            &mut pty,
            Duration::from_millis(max_ms.unwrap_or(1500).clamp(100, 10_000)),
        );
        std::io::stdout().write_all(&output)?;
        let _ = control.shutdown(Shutdown::Both);
        return Ok(());
    }

    run_shell_fd_proxy_loop(&mut pty, &control, max_ms.unwrap_or(300_000))?;
    let _ = control.shutdown(Shutdown::Both);
    Ok(())
}

#[cfg(all(unix, target_os = "linux"))]
fn run_shell_fd_proxy_loop(
    pty: &mut std::fs::File,
    control: &std::os::unix::net::UnixStream,
    max_ms: u64,
) -> AppResult<()> {
    use std::os::fd::AsRawFd;

    let deadline = Instant::now() + Duration::from_millis(max_ms.clamp(100, 300_000));
    let mut buffer = [0u8; 4096];
    let mut stdout = std::io::stdout().lock();
    while Instant::now() < deadline {
        loop {
            match pty.read(&mut buffer) {
                Ok(0) => return Ok(()),
                Ok(count) => {
                    stdout.write_all(&buffer[..count])?;
                    stdout.flush()?;
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(error) if shell_fd_proxy_read_is_eof(&error) => return Ok(()),
                Err(error) => return Err(error.into()),
            }
        }
        while event::poll(Duration::from_millis(1))? {
            match event::read()? {
                Event::Key(key) if shell_attach_interactive_is_detach_key(key) => {
                    let _ = control.shutdown(Shutdown::Both);
                    return Ok(());
                }
                Event::Key(key) => {
                    if let Some(input) = shell_attach_interactive_key_input(key) {
                        pty.write_all(input.as_bytes())?;
                        pty.flush()?;
                    }
                }
                Event::Paste(input) => {
                    if !input.is_empty() {
                        pty.write_all(input.as_bytes())?;
                        pty.flush()?;
                    }
                }
                Event::Resize(cols, rows) => {
                    shell_fd_proxy_set_winsize(pty.as_raw_fd(), rows, cols)?;
                }
                _ => {}
            }
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    Ok(())
}

#[cfg(all(unix, target_os = "linux"))]
fn read_shell_fd_proxy_output_for(pty: &mut std::fs::File, duration: Duration) -> Vec<u8> {
    let deadline = Instant::now() + duration;
    let mut output = Vec::new();
    let mut buffer = [0u8; 4096];
    while Instant::now() < deadline {
        match pty.read(&mut buffer) {
            Ok(0) => break,
            Ok(count) => output.extend_from_slice(&buffer[..count]),
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
            Err(error) if shell_fd_proxy_read_is_eof(&error) => break,
            Err(_) => break,
        }
    }
    output
}

#[cfg(all(unix, target_os = "linux"))]
fn shell_fd_proxy_read_is_eof(error: &std::io::Error) -> bool {
    const LINUX_EIO: i32 = 5;
    matches!(
        error.kind(),
        std::io::ErrorKind::UnexpectedEof | std::io::ErrorKind::BrokenPipe
    ) || error.raw_os_error() == Some(LINUX_EIO)
}

#[cfg(not(all(unix, target_os = "linux")))]
fn run_shell_fd_proxy(_args: AgentsShellArgs) -> AppResult<()> {
    Err(app_error(
        "agents shell fd-proxy currently requires Linux SCM_RIGHTS PTY fd handoff",
    ))
}

fn shell_attach_stream_request_json(args: &ShellAttachStreamFollow<'_>) -> JsonValue {
    let mut request = agents_shell_request_json(&AgentsShellArgs {
        action: AgentsShellAction::Attach {
            task_id: args.task_id.to_string(),
            cursor: args.cursor,
            wait_ms: args.wait_ms,
            limit_bytes: args.limit_bytes,
            tail: args.tail,
            follow: false,
            interactive: false,
            raw: false,
            poll_ms: None,
            max_ms: None,
        },
        json: false,
    });
    if let JsonValue::Object(root) = &mut request {
        root.insert(
            "method".to_string(),
            JsonValue::String("attach_stream".to_string()),
        );
        if let Some(poll_ms) = args.poll_ms {
            root.insert(
                "poll_ms".to_string(),
                JsonValue::Number(poll_ms.to_string()),
            );
        }
        if let Some(max_ms) = args.max_ms {
            root.insert("max_ms".to_string(), JsonValue::Number(max_ms.to_string()));
        }
    }
    request
}

#[cfg(unix)]
fn open_shell_supervisor_cli_stream(
    cwd: &Path,
    request: &JsonValue,
) -> AppResult<BufReader<std::os::unix::net::UnixStream>> {
    let stream = open_shell_supervisor_cli_raw_stream(cwd, request)?;
    Ok(BufReader::new(stream))
}

#[cfg(unix)]
fn open_shell_supervisor_cli_raw_stream(
    cwd: &Path,
    request: &JsonValue,
) -> AppResult<std::os::unix::net::UnixStream> {
    use std::os::unix::net::UnixStream;

    let socket = cwd.join(".dscode/shell-supervisor/supervisor.sock");
    let mut stream = UnixStream::connect(&socket).map_err(|error| {
        app_error(format!(
            "shell supervisor socket is not active at {}: {error}. Start it with `deepseek agents shell-supervisor --json`.",
            socket.display()
        ))
    })?;
    stream.write_all(json_value_to_string(request).as_bytes())?;
    stream.write_all(b"\n")?;
    stream.flush()?;
    Ok(stream)
}

#[cfg(windows)]
fn open_shell_supervisor_cli_stream(
    cwd: &Path,
    request: &JsonValue,
) -> AppResult<BufReader<std::net::TcpStream>> {
    let stream = open_shell_supervisor_cli_tcp_stream(cwd, request)?;
    Ok(BufReader::new(stream))
}

#[cfg(windows)]
fn open_shell_supervisor_cli_tcp_stream(
    cwd: &Path,
    request: &JsonValue,
) -> AppResult<std::net::TcpStream> {
    use std::net::TcpStream;

    let endpoint = read_shell_supervisor_tcp_endpoint(cwd)?;
    let address = endpoint.parse::<std::net::SocketAddr>().map_err(|error| {
        app_error(format!(
            "shell supervisor tcp endpoint is invalid: tcp://{endpoint}: {error}"
        ))
    })?;
    let mut stream =
        TcpStream::connect_timeout(&address, Duration::from_millis(1_000)).map_err(|error| {
            app_error(format!(
                "shell supervisor tcp endpoint is not active at tcp://{endpoint}: {error}. Start it with `deepseek agents shell-supervisor --json`."
            ))
        })?;
    stream
        .set_write_timeout(Some(Duration::from_millis(1_000)))
        .map_err(|error| {
            app_error(format!(
                "shell supervisor write timeout setup failed: {error}"
            ))
        })?;
    stream.write_all(json_value_to_string(request).as_bytes())?;
    stream.write_all(b"\n")?;
    stream.flush()?;
    Ok(stream)
}

#[cfg(windows)]
fn shell_supervisor_tcp_endpoint_path(cwd: &Path) -> PathBuf {
    cwd.join(".dscode/shell-supervisor/supervisor.tcp")
}

#[cfg(windows)]
fn read_shell_supervisor_tcp_endpoint(cwd: &Path) -> AppResult<String> {
    let path = shell_supervisor_tcp_endpoint_path(cwd);
    let raw = std::fs::read_to_string(&path).map_err(|error| {
        app_error(format!(
            "shell supervisor tcp endpoint is not configured at {}: {error}. Start it with `deepseek agents shell-supervisor --json`.",
            path.display()
        ))
    })?;
    shell_supervisor_tcp_endpoint_from_label(&raw)
}

#[cfg(any(test, windows))]
fn shell_supervisor_tcp_endpoint_from_label(label: &str) -> AppResult<String> {
    let value = label.trim();
    let endpoint = value.strip_prefix("tcp://").unwrap_or(value).trim();
    if endpoint.is_empty() {
        return Err(app_error("shell supervisor tcp endpoint is empty"));
    }
    let address = endpoint.parse::<std::net::SocketAddr>().map_err(|error| {
        app_error(format!(
            "shell supervisor tcp endpoint must be tcp://<loopback-ip>:<port>: {error}"
        ))
    })?;
    if !address.ip().is_loopback() {
        return Err(app_error(format!(
            "shell supervisor tcp endpoint must use a loopback address: tcp://{endpoint}"
        )));
    }
    Ok(address.to_string())
}

#[cfg(windows)]
fn shell_supervisor_tcp_endpoint_is_active(endpoint: &str) -> bool {
    let Ok(address) = endpoint.parse::<std::net::SocketAddr>() else {
        return false;
    };
    std::net::TcpStream::connect_timeout(&address, Duration::from_millis(250)).is_ok()
}

#[cfg(not(any(unix, windows)))]
fn open_shell_supervisor_cli_stream(
    _cwd: &Path,
    _request: &JsonValue,
) -> AppResult<BufReader<std::io::Cursor<Vec<u8>>>> {
    Err(app_error(
        "agents shell control currently requires the Unix shell supervisor socket",
    ))
}

fn print_shell_attach_follow_payload(
    response: &BTreeMap<String, JsonValue>,
    summary: &str,
    raw_only: bool,
) -> AppResult<()> {
    let mut stdout = std::io::stdout();
    let _ = shell_attach_write_response_terminal_payload(
        response,
        summary,
        &mut stdout,
        true,
        raw_only,
    )?;
    Ok(())
}

fn shell_attach_summary_next_cursor(summary: &str) -> Option<u64> {
    shell_summary_value(summary, "next_cursor")
        .or_else(|| shell_summary_value(summary, "next_offset"))
        .and_then(|value| value.parse::<u64>().ok())
}

fn shell_attach_summary_terminal_payload(summary: &str) -> Option<&str> {
    shell_summary_section_payload(summary, "terminal")
}

fn shell_attach_write_response_terminal_payload<W: Write>(
    response: &BTreeMap<String, JsonValue>,
    summary: &str,
    writer: &mut W,
    fallback_newline: bool,
    raw_only: bool,
) -> AppResult<bool> {
    if shell_attach_write_response_raw_terminal_payload(response, writer)? {
        return Ok(true);
    }
    shell_attach_write_terminal_payload(summary, writer, fallback_newline, raw_only)
}

fn shell_attach_write_response_raw_terminal_payload<W: Write>(
    response: &BTreeMap<String, JsonValue>,
    writer: &mut W,
) -> AppResult<bool> {
    let Some(events) = response.get("terminal_raw_outputs").and_then(json_as_array) else {
        return Ok(false);
    };
    let mut wrote = false;
    for event in events {
        let Some(object) = json_as_object(event) else {
            continue;
        };
        let kind = object
            .get("kind")
            .and_then(json_as_string)
            .unwrap_or("output");
        if kind != "output" {
            continue;
        }
        let Some(encoded) = object
            .get("raw_base64")
            .or_else(|| object.get("rawBase64"))
            .and_then(json_as_string)
        else {
            continue;
        };
        let bytes = decode_shell_base64(encoded)?;
        if !bytes.is_empty() {
            writer.write_all(&bytes)?;
            wrote = true;
        }
    }
    if wrote {
        writer.flush()?;
    }
    Ok(wrote)
}

fn shell_attach_write_terminal_payload<W: Write>(
    summary: &str,
    writer: &mut W,
    fallback_newline: bool,
    raw_only: bool,
) -> AppResult<bool> {
    if shell_attach_write_raw_terminal_payload(summary, writer)? {
        return Ok(true);
    }
    if raw_only && shell_summary_value(summary, "mode") == Some("terminal_event_attach") {
        return Ok(false);
    }
    let Some(payload) = shell_attach_summary_terminal_payload(summary) else {
        return Ok(false);
    };
    writer.write_all(payload.as_bytes())?;
    if fallback_newline {
        writer.write_all(b"\n")?;
    }
    writer.flush()?;
    Ok(!payload.is_empty())
}

fn shell_attach_write_raw_terminal_payload<W: Write>(
    summary: &str,
    writer: &mut W,
) -> AppResult<bool> {
    let events = shell_attach_raw_output_events_from_summary(summary);
    if events.is_empty() {
        return Ok(false);
    }
    shell_attach_write_raw_output_events(&events, writer)
}

fn shell_attach_write_raw_output_events<W: Write>(
    events: &[(u64, String)],
    writer: &mut W,
) -> AppResult<bool> {
    let mut wrote = false;
    for (_, encoded) in events {
        let bytes = decode_shell_base64(encoded)?;
        if !bytes.is_empty() {
            writer.write_all(&bytes)?;
            wrote = true;
        }
    }
    if wrote {
        writer.flush()?;
    }
    Ok(wrote)
}

fn shell_attach_raw_output_events_from_summary(summary: &str) -> Vec<(u64, String)> {
    let Some(payload) = shell_summary_section_payload(summary, "terminal_raw_base64") else {
        return Vec::new();
    };
    payload
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            let seq = parts.next()?.parse::<u64>().ok()?;
            let kind = parts.next()?;
            if kind != "output" {
                return None;
            }
            let encoded = parts.next()?;
            Some((seq, encoded.to_string()))
        })
        .collect()
}

fn shell_attach_raw_outputs_json_from_summary(summary: &str) -> Option<JsonValue> {
    let events = shell_attach_raw_output_events_from_summary(summary);
    if events.is_empty() {
        return None;
    }
    Some(JsonValue::Array(
        events
            .into_iter()
            .map(|(seq, raw_base64)| {
                JsonValue::Object(BTreeMap::from([
                    ("seq".to_string(), JsonValue::Number(seq.to_string())),
                    ("kind".to_string(), JsonValue::String("output".to_string())),
                    ("raw_base64".to_string(), JsonValue::String(raw_base64)),
                ]))
            })
            .collect(),
    ))
}

#[cfg(unix)]
fn encode_shell_base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0];
        let b1 = *chunk.get(1).unwrap_or(&0);
        let b2 = *chunk.get(2).unwrap_or(&0);
        out.push(TABLE[(b0 >> 2) as usize] as char);
        out.push(TABLE[(((b0 & 0b0000_0011) << 4) | (b1 >> 4)) as usize] as char);
        if chunk.len() > 1 {
            out.push(TABLE[(((b1 & 0b0000_1111) << 2) | (b2 >> 6)) as usize] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(TABLE[(b2 & 0b0011_1111) as usize] as char);
        } else {
            out.push('=');
        }
    }
    out
}

fn decode_shell_base64(value: &str) -> AppResult<Vec<u8>> {
    let encoded = value.trim();
    if encoded.is_empty() {
        return Ok(Vec::new());
    }
    if encoded.len() % 4 != 0 {
        return Err(app_error("terminal raw_base64 payload has invalid length"));
    }
    let mut out = Vec::with_capacity(encoded.len() / 4 * 3);
    for (index, chunk) in encoded.as_bytes().chunks(4).enumerate() {
        let a = shell_base64_value(chunk[0])?;
        let b = shell_base64_value(chunk[1])?;
        let c_pad = chunk[2] == b'=';
        let d_pad = chunk[3] == b'=';
        if c_pad && !d_pad {
            return Err(app_error("terminal raw_base64 payload has invalid padding"));
        }
        if (c_pad || d_pad) && index + 1 != encoded.len() / 4 {
            return Err(app_error("terminal raw_base64 payload has invalid padding"));
        }
        let c = if c_pad {
            0
        } else {
            shell_base64_value(chunk[2])?
        };
        let d = if d_pad {
            0
        } else {
            shell_base64_value(chunk[3])?
        };
        out.push((a << 2) | (b >> 4));
        if !c_pad {
            out.push(((b & 0b0000_1111) << 4) | (c >> 2));
        }
        if !d_pad {
            out.push(((c & 0b0000_0011) << 6) | d);
        }
    }
    Ok(out)
}

fn shell_base64_value(byte: u8) -> AppResult<u8> {
    match byte {
        b'A'..=b'Z' => Ok(byte - b'A'),
        b'a'..=b'z' => Ok(byte - b'a' + 26),
        b'0'..=b'9' => Ok(byte - b'0' + 52),
        b'+' => Ok(62),
        b'/' => Ok(63),
        _ => Err(app_error(
            "terminal raw_base64 payload contains invalid byte",
        )),
    }
}

fn shell_summary_section_payload<'a>(summary: &'a str, section: &str) -> Option<&'a str> {
    let marker = format!("{section}:\n");
    let (_, rest) = summary.split_once(&marker)?;
    let mut end = rest.len();
    for other in ["data", "terminal_raw_base64", "terminal"] {
        if other == section {
            continue;
        }
        let other_marker = format!("\n{other}:\n");
        if let Some(index) = rest.find(&other_marker) {
            end = end.min(index);
        }
    }
    let payload = rest[..end].trim_end_matches('\n');
    (!payload.is_empty()).then_some(payload)
}

fn shell_summary_value<'a>(summary: &'a str, key: &str) -> Option<&'a str> {
    let prefix = format!("{key}: ");
    summary
        .lines()
        .find_map(|line| line.strip_prefix(&prefix).map(str::trim))
}

fn agents_shell_request_json(args: &AgentsShellArgs) -> JsonValue {
    let mut object = BTreeMap::new();
    match &args.action {
        AgentsShellAction::Status => {
            object.insert(
                "method".to_string(),
                JsonValue::String("status".to_string()),
            );
        }
        AgentsShellAction::Show => {
            object.insert("method".to_string(), JsonValue::String("show".to_string()));
        }
        AgentsShellAction::Start {
            command,
            cwd,
            tty,
            tty_rows,
            tty_cols,
        } => {
            object.insert("method".to_string(), JsonValue::String("start".to_string()));
            object.insert("command".to_string(), JsonValue::String(command.clone()));
            if let Some(cwd) = cwd {
                object.insert("cwd".to_string(), JsonValue::String(cwd.clone()));
            }
            if *tty {
                object.insert("tty".to_string(), JsonValue::Bool(true));
            }
            insert_optional_number(&mut object, "tty_rows", *tty_rows);
            insert_optional_number(&mut object, "tty_cols", *tty_cols);
        }
        AgentsShellAction::Wait {
            task_id,
            timeout_ms,
        } => {
            object.insert("method".to_string(), JsonValue::String("wait".to_string()));
            object.insert("task_id".to_string(), JsonValue::String(task_id.clone()));
            insert_optional_number(&mut object, "timeout_ms", *timeout_ms);
        }
        AgentsShellAction::Replay {
            task_id,
            stream,
            cursor,
            offset,
            limit_bytes,
            tail,
        } => {
            object.insert(
                "method".to_string(),
                JsonValue::String("replay".to_string()),
            );
            object.insert("task_id".to_string(), JsonValue::String(task_id.clone()));
            if let Some(stream) = stream {
                object.insert("stream".to_string(), JsonValue::String(stream.clone()));
            }
            insert_optional_number(&mut object, "cursor", *cursor);
            insert_optional_number(&mut object, "offset", *offset);
            insert_optional_number(&mut object, "limit_bytes", *limit_bytes);
            if *tail {
                object.insert("tail".to_string(), JsonValue::Bool(true));
            }
        }
        AgentsShellAction::Attach {
            task_id,
            cursor,
            wait_ms,
            limit_bytes,
            tail,
            follow: _,
            interactive: _,
            raw: _,
            poll_ms: _,
            max_ms: _,
        } => {
            object.insert(
                "method".to_string(),
                JsonValue::String("attach".to_string()),
            );
            object.insert("task_id".to_string(), JsonValue::String(task_id.clone()));
            insert_optional_number(&mut object, "cursor", *cursor);
            insert_optional_number(&mut object, "wait_ms", *wait_ms);
            insert_optional_number(&mut object, "limit_bytes", *limit_bytes);
            if *tail {
                object.insert("tail".to_string(), JsonValue::Bool(true));
            }
        }
        AgentsShellAction::ByteStream {
            task_id,
            cursor,
            wait_ms,
            limit_bytes,
            tail,
            input,
            close_stdin,
            tty_rows,
            tty_cols,
            poll_ms,
            max_ms,
            max_events,
            raw_proxy,
            terminal_proxy: _,
        } => {
            object.insert(
                "method".to_string(),
                JsonValue::String("byte_stream".to_string()),
            );
            object.insert("task_id".to_string(), JsonValue::String(task_id.clone()));
            insert_optional_number(&mut object, "cursor", *cursor);
            insert_optional_number(&mut object, "wait_ms", *wait_ms);
            insert_optional_number(&mut object, "limit_bytes", *limit_bytes);
            if *tail {
                object.insert("tail".to_string(), JsonValue::Bool(true));
            }
            if let Some(input) = input {
                object.insert("input".to_string(), JsonValue::String(input.clone()));
            }
            if *close_stdin {
                object.insert("close_stdin".to_string(), JsonValue::Bool(true));
            }
            insert_optional_number(&mut object, "tty_rows", *tty_rows);
            insert_optional_number(&mut object, "tty_cols", *tty_cols);
            insert_optional_number(&mut object, "poll_ms", *poll_ms);
            insert_optional_number(&mut object, "max_ms", *max_ms);
            insert_optional_number(&mut object, "max_events", *max_events);
            if *raw_proxy {
                object.insert("raw_proxy".to_string(), JsonValue::Bool(true));
            }
        }
        AgentsShellAction::FdProxy {
            task_id,
            tty_rows,
            tty_cols,
            max_ms,
        } => {
            object.insert(
                "method".to_string(),
                JsonValue::String("pty_fd".to_string()),
            );
            object.insert("task_id".to_string(), JsonValue::String(task_id.clone()));
            insert_optional_number(&mut object, "tty_rows", *tty_rows);
            insert_optional_number(&mut object, "tty_cols", *tty_cols);
            insert_optional_number(&mut object, "max_ms", *max_ms);
        }
        AgentsShellAction::Stdin {
            task_id,
            input,
            close_stdin,
            timeout_ms,
        } => {
            object.insert("method".to_string(), JsonValue::String("stdin".to_string()));
            object.insert("task_id".to_string(), JsonValue::String(task_id.clone()));
            if let Some(input) = input {
                object.insert("input".to_string(), JsonValue::String(input.clone()));
            }
            if *close_stdin {
                object.insert("close_stdin".to_string(), JsonValue::Bool(true));
            }
            insert_optional_number(&mut object, "timeout_ms", *timeout_ms);
        }
        AgentsShellAction::Resize {
            task_id,
            tty_rows,
            tty_cols,
        } => {
            object.insert(
                "method".to_string(),
                JsonValue::String("resize".to_string()),
            );
            object.insert("task_id".to_string(), JsonValue::String(task_id.clone()));
            object.insert(
                "tty_rows".to_string(),
                JsonValue::Number(tty_rows.to_string()),
            );
            object.insert(
                "tty_cols".to_string(),
                JsonValue::Number(tty_cols.to_string()),
            );
        }
        AgentsShellAction::Cancel { task_id, all } => {
            object.insert(
                "method".to_string(),
                JsonValue::String("cancel".to_string()),
            );
            if *all {
                object.insert("all".to_string(), JsonValue::Bool(true));
            }
            if let Some(task_id) = task_id {
                object.insert("task_id".to_string(), JsonValue::String(task_id.clone()));
            }
        }
        AgentsShellAction::Shutdown => {
            object.insert(
                "method".to_string(),
                JsonValue::String("shutdown".to_string()),
            );
        }
    }
    JsonValue::Object(object)
}

fn insert_optional_number(object: &mut BTreeMap<String, JsonValue>, key: &str, value: Option<u64>) {
    if let Some(value) = value {
        object.insert(key.to_string(), JsonValue::Number(value.to_string()));
    }
}

#[cfg(unix)]
fn send_shell_supervisor_cli_request(cwd: &Path, request: &JsonValue) -> AppResult<JsonValue> {
    use std::os::unix::net::UnixStream;

    let socket = cwd.join(".dscode/shell-supervisor/supervisor.sock");
    let mut stream = UnixStream::connect(&socket).map_err(|error| {
        app_error(format!(
            "shell supervisor socket is not active at {}: {error}. Start it with `deepseek agents shell-supervisor --json`.",
            socket.display()
        ))
    })?;
    stream.write_all(json_value_to_string(request).as_bytes())?;
    stream.write_all(b"\n")?;
    stream.flush()?;
    let mut line = String::new();
    BufReader::new(stream).read_line(&mut line)?;
    parse_json_value(line.trim()).map_err(|error| {
        app_error(format!(
            "shell supervisor returned invalid response JSON: {error}"
        ))
    })
}

#[cfg(windows)]
fn send_shell_supervisor_cli_request(cwd: &Path, request: &JsonValue) -> AppResult<JsonValue> {
    let stream = open_shell_supervisor_cli_tcp_stream(cwd, request)?;
    stream
        .set_read_timeout(Some(Duration::from_millis(5_000)))
        .map_err(|error| {
            app_error(format!(
                "shell supervisor read timeout setup failed: {error}"
            ))
        })?;
    let mut line = String::new();
    BufReader::new(stream).read_line(&mut line)?;
    parse_json_value(line.trim()).map_err(|error| {
        app_error(format!(
            "shell supervisor returned invalid response JSON: {error}"
        ))
    })
}

#[cfg(not(any(unix, windows)))]
fn send_shell_supervisor_cli_request(_cwd: &Path, _request: &JsonValue) -> AppResult<JsonValue> {
    Err(app_error(
        "agents shell control currently requires the Unix shell supervisor socket",
    ))
}

fn print_shell_control_response(args: &AgentsShellArgs, response: JsonValue) -> AppResult<()> {
    if args.json {
        println!("{}", json_value_to_string(&response));
        return Ok(());
    }
    let Some(object) = json_as_object(&response) else {
        return Err(app_error("shell supervisor response must be a JSON object"));
    };
    let status = object
        .get("status")
        .and_then(json_as_string)
        .unwrap_or("unknown");
    if status == "error" || status == "unsupported" {
        let error = object
            .get("error")
            .and_then(json_as_string)
            .unwrap_or("shell supervisor request failed");
        return Err(app_error(error.to_string()));
    }
    let summary_key = match args.action {
        AgentsShellAction::Show => Some("job_inventory"),
        AgentsShellAction::Start { .. } => Some("start_summary"),
        AgentsShellAction::Wait { .. } => Some("wait_summary"),
        AgentsShellAction::Replay { .. } => Some("replay_summary"),
        AgentsShellAction::Attach { .. } => Some("attach_summary"),
        AgentsShellAction::ByteStream { .. } => Some("attach_summary"),
        AgentsShellAction::FdProxy { .. } => None,
        AgentsShellAction::Stdin { .. } => Some("stdin_summary"),
        AgentsShellAction::Resize { .. } => Some("resize_summary"),
        AgentsShellAction::Cancel { .. } => Some("cancel_summary"),
        AgentsShellAction::Status | AgentsShellAction::Shutdown => None,
    };
    if let Some(key) = summary_key {
        if let Some(summary) = object.get(key).and_then(json_as_string) {
            println!("{summary}");
            return Ok(());
        }
    }
    print_shell_control_status(object);
    Ok(())
}

fn print_shell_control_status(object: &BTreeMap<String, JsonValue>) {
    let method = object
        .get("method")
        .and_then(json_as_string)
        .unwrap_or("status");
    let status = object
        .get("status")
        .and_then(json_as_string)
        .unwrap_or("unknown");
    println!("shell supervisor {method}: {status}");
    for key in [
        "cwd",
        "supervisor_pid",
        "supervisor_socket",
        "supervisor_epoch",
        "protocol",
        "native_pty",
        "active_jobs",
    ] {
        if let Some(value) = object.get(key) {
            println!("{key}: {}", shell_control_scalar(value));
        }
    }
}

fn shell_control_scalar(value: &JsonValue) -> String {
    match value {
        JsonValue::String(value) | JsonValue::Number(value) => value.clone(),
        JsonValue::Bool(value) => value.to_string(),
        JsonValue::Null => "null".to_string(),
        JsonValue::Array(_) | JsonValue::Object(_) => json_value_to_string(value),
    }
}

fn list_agents(config_dir: &str) -> AppResult<()> {
    let results = load_default_agents(config_dir);
    if results.is_empty() {
        println!("No subagents configured.");
        println!("Add project agents under .dscode/agents/*.md");
        println!("Add user agents under ~/.config/dscode/agents/*.md");
        return Ok(());
    }

    println!("Subagents:");
    for result in results {
        match result {
            Ok(agent) => println!(
                "- {} {}: {} ({})",
                agent.source.label(),
                agent.name,
                agent.description,
                agent.path.display()
            ),
            Err(error) => println!("- error {}: {}", error.path.display(), error.message),
        }
    }
    Ok(())
}

fn show_agent(config_dir: &str, name: &str) -> AppResult<()> {
    let agent = crate::core::agents::find_agent(config_dir, name)
        .map_err(|error| app_error(format!("{}: {}", error.path.display(), error.message)))?;

    println!("Name: {}", agent.name);
    println!("Source: {}", agent.source.label());
    println!("Path: {}", agent.path.display());
    println!("Description: {}", agent.description);
    println!(
        "Tools: {}",
        if agent.tools.is_empty() {
            "all".to_string()
        } else {
            agent.tools.join(", ")
        }
    );
    println!(
        "Model: {}",
        agent.model.as_deref().unwrap_or("default configured model")
    );
    println!();
    println!("{}", agent.prompt);
    Ok(())
}

fn validate_agents(config_dir: &str, path: Option<String>) -> AppResult<()> {
    let results = if let Some(path) = path {
        vec![load_agent_file(Path::new(&path), AgentSource::File)]
    } else {
        load_default_agents(config_dir)
    };

    if results.is_empty() {
        println!("No agent files found.");
        return Ok(());
    }

    let mut failed = 0usize;
    for result in &results {
        match result {
            Ok(agent) => println!("OK {} name={}", agent.path.display(), agent.name),
            Err(error) => {
                failed += 1;
                println!("ERR {} {}", error.path.display(), error.message);
            }
        }
    }

    if failed > 0 {
        return Err(app_error(format!(
            "agent validation failed for {failed} file{}",
            if failed == 1 { "" } else { "s" }
        )));
    }
    Ok(())
}

fn run_runtime_task(
    config: AppConfig,
    task_id: &str,
    budget: Option<usize>,
    json: bool,
) -> AppResult<()> {
    let store = RuntimeStore::new(PathBuf::from(&config.workspace.config_dir).join("runtime"));
    let rollback_store =
        RollbackStore::new(PathBuf::from(&config.workspace.config_dir).join("rollback"));
    let task = store.load_task(task_id)?;
    let thread_id = task
        .thread_id
        .clone()
        .ok_or_else(|| app_error("agents run-task requires a task linked to a runtime thread"))?;
    let thread = store.load_thread(&thread_id)?;
    let runner_id = format!("local-runner-{}", std::process::id());
    let task = store.claim_task(task_id, runner_id.clone())?;
    if json {
        println!(
            "{}",
            json_value_to_string(&runner_event(
                "task_claimed",
                &task.id,
                &thread.id,
                Some(&runner_id),
                None,
            ))
        );
    } else {
        println!("claimed runtime task: {}", task.id);
        println!("thread: {}", thread.id);
    }

    let workspace = PathBuf::from(&thread.workspace);
    let rollback_snapshot_id = rollback_store
        .create_snapshot(&workspace, format!("runtime task rollback: {}", task.id))
        .ok()
        .map(|snapshot| snapshot.id);
    let cwd_guard = match crate::util::cwd::CwdGuard::enter(&workspace) {
        Ok(guard) => guard,
        Err(error) => {
            record_runtime_task_failure(&store, &task, &thread, &error.to_string())?;
            if json {
                println!(
                    "{}",
                    json_value_to_string(&runner_event(
                        "task_failed",
                        &task.id,
                        &thread.id,
                        None,
                        Some(&error.to_string()),
                    ))
                );
            }
            return Err(error);
        }
    };
    let run_result = run_runtime_task_loop(&config, &store, &task, &thread, budget, json);
    cwd_guard.restore()?;

    match run_result {
        Ok(result) => {
            let assistant_turn_id = record_runtime_task_result(&store, &task, &thread, &result)?;
            if let Some(snapshot_id) = rollback_snapshot_id {
                let _ = rollback_store.bind_snapshot_runtime(
                    &snapshot_id,
                    Some(&thread.id),
                    Some(&assistant_turn_id),
                );
            }
            if json {
                println!(
                    "{}",
                    json_value_to_string(&runner_event(
                        "task_completed",
                        &task.id,
                        &thread.id,
                        None,
                        Some(&result.final_message),
                    ))
                );
            }
            Ok(())
        }
        Err(error) => {
            record_runtime_task_failure(&store, &task, &thread, &error.to_string())?;
            if json {
                println!(
                    "{}",
                    json_value_to_string(&runner_event(
                        "task_failed",
                        &task.id,
                        &thread.id,
                        None,
                        Some(&error.to_string()),
                    ))
                );
            }
            Err(error)
        }
    }
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct RuntimeDaemonTick {
    triggered_automations: usize,
    executed_tasks: usize,
    executed_rlm_turns: usize,
    recovered_rlm_turns: usize,
    compacted_threads: usize,
    failed_automations: usize,
    failed_tasks: usize,
    failed_rlm_turns: usize,
    failed_rlm_recoveries: usize,
    failed_compactions: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct RuntimeDaemonCompactionSettings {
    threshold_tokens: u64,
    keep_tail_turns: usize,
}

fn runtime_daemon_compaction_settings(config: &AppConfig) -> RuntimeDaemonCompactionSettings {
    RuntimeDaemonCompactionSettings {
        threshold_tokens: config.runtime.daemon_compaction_threshold_tokens,
        keep_tail_turns: config
            .runtime
            .daemon_compaction_keep_tail_turns
            .clamp(1, 200),
    }
}

fn run_runtime_daemon(
    config: AppConfig,
    budget: Option<usize>,
    interval_ms: u64,
    once: bool,
    json: bool,
) -> AppResult<()> {
    let runtime_root = PathBuf::from(&config.workspace.config_dir).join("runtime");
    let store = RuntimeStore::new(runtime_root.clone());
    let interval = Duration::from_millis(interval_ms.max(100));
    if !json {
        println!("runtime daemon watching {}", runtime_root.display());
        println!("poll interval: {}ms", interval.as_millis());
    }

    loop {
        let tick = run_runtime_daemon_tick(&config, &store, budget, json)?;
        if json {
            println!("{}", json_value_to_string(&daemon_tick_event(&tick)));
        } else if tick.triggered_automations > 0
            || tick.executed_tasks > 0
            || tick.executed_rlm_turns > 0
            || tick.recovered_rlm_turns > 0
            || tick.compacted_threads > 0
            || tick.failed_automations > 0
            || tick.failed_tasks > 0
            || tick.failed_rlm_turns > 0
            || tick.failed_rlm_recoveries > 0
            || tick.failed_compactions > 0
        {
            println!(
                "daemon tick: triggered={} executed={} rlm_executed={} rlm_recovered={} compacted={} automation_errors={} task_errors={} rlm_errors={} rlm_recovery_errors={} compaction_errors={}",
                tick.triggered_automations,
                tick.executed_tasks,
                tick.executed_rlm_turns,
                tick.recovered_rlm_turns,
                tick.compacted_threads,
                tick.failed_automations,
                tick.failed_tasks,
                tick.failed_rlm_turns,
                tick.failed_rlm_recoveries,
                tick.failed_compactions
            );
        }

        if once {
            return Ok(());
        }
        std::thread::sleep(interval);
    }
}

fn run_rlm_status(config: AppConfig, args: AgentsRlmStatusArgs) -> AppResult<()> {
    let output = RlmLiveStatusTool { config }.execute(rlm_status_tool_input(&args))?;
    print_rlm_cli_output(&output.summary, args.json);
    Ok(())
}

fn run_rlm_events(config: AppConfig, args: AgentsRlmEventsArgs) -> AppResult<()> {
    let output = RlmLiveEventsTool { config }.execute(rlm_events_tool_input(&args))?;
    print_rlm_cli_output(&output.summary, args.json);
    Ok(())
}

fn run_rlm_wait(config: AppConfig, args: AgentsRlmWaitArgs) -> AppResult<()> {
    let output = RlmLiveWaitTool { config }.execute(rlm_wait_tool_input(&args))?;
    print_rlm_cli_output(&output.summary, args.json);
    Ok(())
}

fn run_rlm_cancel(config: AppConfig, args: AgentsRlmCancelArgs) -> AppResult<()> {
    let output = RlmLiveCancelTool { config }.execute(rlm_cancel_tool_input(&args))?;
    print_rlm_cli_output(&output.summary, args.json);
    Ok(())
}

fn run_rlm_recover(config: AppConfig, args: AgentsRlmRecoverArgs) -> AppResult<()> {
    let output = RlmLiveRecoverTool { config }.execute(rlm_recover_tool_input(&args))?;
    print_rlm_cli_output(&output.summary, args.json);
    Ok(())
}

fn run_rlm_stop(config: AppConfig, args: AgentsRlmStopArgs) -> AppResult<()> {
    let output = RlmLiveStopTool { config }.execute(rlm_stop_tool_input(&args))?;
    print_rlm_cli_output(&output.summary, args.json);
    Ok(())
}

fn run_rlm_run_next(config: AppConfig, args: AgentsRlmRunNextArgs) -> AppResult<()> {
    let output = RlmLiveRunNextTool {
        config,
        parent_depth: 0,
    }
    .execute(rlm_run_next_tool_input(&args))?;
    print_rlm_cli_output(&output.summary, args.json);
    Ok(())
}

fn run_rlm_drain(config: AppConfig, args: AgentsRlmDrainArgs) -> AppResult<()> {
    let output = RlmLiveDrainTool {
        config,
        parent_depth: 0,
    }
    .execute(rlm_drain_tool_input(&args))?;
    print_rlm_cli_output(&output.summary, args.json);
    Ok(())
}

fn rlm_status_tool_input(args: &AgentsRlmStatusArgs) -> ToolInput {
    let mut input = ToolInput::new();
    if let Some(session_id) = &args.session_id {
        input = input.with_arg("session_id", session_id.clone());
    }
    if let Some(limit) = args.limit {
        input = input.with_arg("limit", limit.to_string());
    }
    input
}

fn rlm_events_tool_input(args: &AgentsRlmEventsArgs) -> ToolInput {
    let mut input = ToolInput::new().with_arg("session_id", args.session_id.clone());
    if let Some(cursor) = args.cursor {
        input = input.with_arg("cursor", cursor.to_string());
    }
    if let Some(limit) = args.limit {
        input = input.with_arg("limit", limit.to_string());
    }
    input
}

fn rlm_wait_tool_input(args: &AgentsRlmWaitArgs) -> ToolInput {
    let mut input = ToolInput::new().with_arg("session_id", args.session_id.clone());
    if let Some(cursor) = args.cursor {
        input = input.with_arg("cursor", cursor.to_string());
    }
    if let Some(limit) = args.limit {
        input = input.with_arg("limit", limit.to_string());
    }
    if let Some(timeout_ms) = args.timeout_ms {
        input = input.with_arg("timeout_ms", timeout_ms.to_string());
    }
    if let Some(poll_interval_ms) = args.poll_interval_ms {
        input = input.with_arg("poll_interval_ms", poll_interval_ms.to_string());
    }
    input
}

fn rlm_cancel_tool_input(args: &AgentsRlmCancelArgs) -> ToolInput {
    let mut input = ToolInput::new().with_arg("session_id", args.session_id.clone());
    if let Some(task_id) = &args.task_id {
        input = input.with_arg("task_id", task_id.clone());
    }
    if args.all {
        input = input.with_arg("all", "true");
    }
    if args.force {
        input = input.with_arg("force", "true");
    }
    if let Some(reason) = &args.reason {
        input = input.with_arg("reason", reason.clone());
    }
    input
}

fn rlm_recover_tool_input(args: &AgentsRlmRecoverArgs) -> ToolInput {
    let mut input = ToolInput::new();
    if let Some(session_id) = &args.session_id {
        input = input.with_arg("session_id", session_id.clone());
    }
    if args.all {
        input = input.with_arg("all", "true");
    }
    if let Some(mode) = &args.mode {
        input = input.with_arg("mode", mode.clone());
    }
    if args.dry_run {
        input = input.with_arg("dry_run", "true");
    }
    if args.force {
        input = input.with_arg("force", "true");
    }
    if let Some(limit) = args.limit {
        input = input.with_arg("limit", limit.to_string());
    }
    if let Some(reason) = &args.reason {
        input = input.with_arg("reason", reason.clone());
    }
    input
}

fn rlm_stop_tool_input(args: &AgentsRlmStopArgs) -> ToolInput {
    let mut input = ToolInput::new().with_arg("session_id", args.session_id.clone());
    if let Some(reason) = &args.reason {
        input = input.with_arg("reason", reason.clone());
    }
    input
}

fn rlm_run_next_tool_input(args: &AgentsRlmRunNextArgs) -> ToolInput {
    let mut input = ToolInput::new().with_arg("session_id", args.session_id.clone());
    if let Some(task_id) = &args.task_id {
        input = input.with_arg("task_id", task_id.clone());
    }
    if args.dry_run {
        input = input.with_arg("dry_run", "true");
    }
    input
}

fn rlm_drain_tool_input(args: &AgentsRlmDrainArgs) -> ToolInput {
    let mut input = ToolInput::new().with_arg("session_id", args.session_id.clone());
    if let Some(max_turns) = args.max_turns {
        input = input.with_arg("max_turns", max_turns.to_string());
    }
    if args.dry_run {
        input = input.with_arg("dry_run", "true");
    }
    input
}

fn print_rlm_cli_output(summary: &str, json: bool) {
    if json {
        println!("{summary}");
        return;
    }
    let Ok(value) = parse_json_value(summary) else {
        println!("{summary}");
        return;
    };
    let Some(root) = json_as_object(&value) else {
        println!("{summary}");
        return;
    };
    if root.get("totals").is_some() {
        let sessions = root
            .get("sessions")
            .and_then(json_as_array)
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        println!("RLM live sessions: {}", sessions.len());
        for session in sessions.iter().take(20) {
            print_rlm_status_line(session);
        }
        if let Some(errors) = root.get("errors").and_then(json_as_array) {
            if !errors.is_empty() {
                println!("errors: {}", errors.len());
            }
        }
        return;
    }
    if let Some(events) = root.get("events").and_then(json_as_array) {
        let session_id = root
            .get("session_id")
            .and_then(json_as_string)
            .unwrap_or("-");
        let cursor = rlm_cli_scalar(root.get("cursor"));
        let next_cursor = rlm_cli_scalar(root.get("next_cursor"));
        println!(
            "RLM events for {session_id}: {} event(s), cursor={cursor}, next_cursor={next_cursor}",
            events.len()
        );
        for event in events.iter().take(20) {
            if let Some(event) = json_as_object(event) {
                let seq = rlm_cli_scalar(event.get("seq"));
                let kind = event.get("kind").and_then(json_as_string).unwrap_or("-");
                let task_id = event.get("task_id").and_then(json_as_string).unwrap_or("-");
                println!("- seq={seq} kind={kind} task={task_id}");
            }
        }
        return;
    }
    if root.get("cancelled_count").is_some()
        || root.get("recovered_count").is_some()
        || root.get("selected_count").is_some()
        || root.get("ran_count").is_some()
        || root.get("task_id").is_some()
    {
        print_rlm_action_summary(root);
        return;
    }
    print_rlm_status_line(&value);
}

fn print_rlm_action_summary(root: &std::collections::BTreeMap<String, JsonValue>) {
    let session_id = root
        .get("session_id")
        .and_then(json_as_string)
        .unwrap_or("-");
    let dry_run = rlm_cli_scalar(root.get("dry_run"));
    if root.get("status").and_then(json_as_string) == Some("stopped") {
        println!(
            "RLM stop {session_id}: cancelled={} queued={} reason={}",
            rlm_cli_scalar(root.get("cancelled_count")),
            rlm_cli_scalar(root.get("queued_turns")),
            rlm_cli_scalar(root.get("reason"))
        );
    } else if root.get("task_id").is_some() && root.get("status").is_some() {
        println!(
            "RLM run-next {session_id}: task={} status={} queued={}",
            rlm_cli_scalar(root.get("task_id")),
            rlm_cli_scalar(root.get("status")),
            rlm_cli_scalar(root.get("queued_turns"))
        );
    } else if root.get("cancelled_count").is_some() {
        println!(
            "RLM cancel {session_id}: cancelled={} active_owner_cancelled={} interrupted={} queued={}",
            rlm_cli_scalar(root.get("cancelled_count")),
            rlm_cli_scalar(root.get("active_owner_cancelled")),
            rlm_cli_scalar(root.get("interrupted")),
            rlm_cli_scalar(root.get("queued_turns"))
        );
    } else if root.get("recovered_count").is_some() {
        println!(
            "RLM recover {session_id}: recovered={} mode={} dry_run={} force={} queued={}",
            rlm_cli_scalar(root.get("recovered_count")),
            rlm_cli_scalar(root.get("mode")),
            dry_run,
            rlm_cli_scalar(root.get("force")),
            rlm_cli_scalar(root.get("queued_turns"))
        );
    } else if root.get("selected_count").is_some() {
        println!(
            "RLM drain {session_id}: selected={} max_turns={} dry_run={}",
            rlm_cli_scalar(root.get("selected_count")),
            rlm_cli_scalar(root.get("max_turns")),
            dry_run
        );
    } else if root.get("ran_count").is_some() {
        println!(
            "RLM drain {session_id}: ran={} queued={} dry_run={}",
            rlm_cli_scalar(root.get("ran_count")),
            rlm_cli_scalar(root.get("queued_turns")),
            dry_run
        );
    }
    if let Some(actions) = root.get("actions").and_then(json_as_array) {
        for action in actions.iter().take(5) {
            if let Some(action) = json_as_object(action) {
                println!(
                    "- task={} action={} reason={}",
                    rlm_cli_scalar(action.get("task_id")),
                    rlm_cli_scalar(action.get("action")),
                    rlm_cli_scalar(action.get("reason"))
                );
            }
        }
    }
}

fn print_rlm_status_line(value: &JsonValue) {
    let Some(root) = json_as_object(value) else {
        println!("{}", json_value_to_string(value));
        return;
    };
    let session_id = root
        .get("session_id")
        .and_then(json_as_string)
        .unwrap_or("-");
    if matches!(root.get("exists"), Some(JsonValue::Bool(false))) {
        println!("RLM live session {session_id}: not found");
        return;
    }
    let status = root.get("status").and_then(json_as_string).unwrap_or("-");
    let queued = rlm_cli_scalar(root.get("queued_turns_runtime"));
    let active = rlm_cli_scalar(root.get("active_turn_id"));
    let owner = root
        .get("daemon_owner")
        .and_then(json_as_string)
        .unwrap_or("-");
    let alive = rlm_cli_scalar(root.get("daemon_alive"));
    println!(
        "RLM live session {session_id}: status={status} queued={queued} active={active} owner={owner} alive={alive}"
    );
    if let Some(actions) = root.get("recommended_actions").and_then(json_as_array) {
        let actions = actions
            .iter()
            .filter_map(json_as_string)
            .take(3)
            .collect::<Vec<_>>();
        if !actions.is_empty() {
            println!("  next: {}", actions.join("; "));
        }
    }
}

fn rlm_cli_scalar(value: Option<&JsonValue>) -> String {
    match value {
        Some(JsonValue::String(value)) => value.clone(),
        Some(JsonValue::Number(value)) => value.clone(),
        Some(JsonValue::Bool(value)) => value.to_string(),
        Some(JsonValue::Null) | None => "-".to_string(),
        Some(value) => json_value_to_string(value),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ServiceTemplate {
    path: &'static str,
    body: String,
}

#[derive(Debug, Clone)]
struct ServiceTemplateConfig {
    kind: AgentsServiceKind,
    out: Option<PathBuf>,
    bin: String,
    workdir: String,
    addr: String,
    interval_ms: u64,
    budget: Option<usize>,
}

fn run_shell_supervisor(args: AgentsShellSupervisorArgs) -> AppResult<()> {
    let cwd = std::env::current_dir()?;
    if args.once {
        let output = ExecShellSupervisorStatusTool
            .execute(ToolInput::new().with_arg("cwd", cwd.display().to_string()))?;
        if args.json {
            let mut object = BTreeMap::new();
            object.insert(
                "kind".to_string(),
                JsonValue::String("deepseek.exec_shell.supervisor_once.v1".to_string()),
            );
            object.insert(
                "cwd".to_string(),
                JsonValue::String(cwd.display().to_string()),
            );
            object.insert("status".to_string(), JsonValue::String(output.summary));
            println!("{}", json_value_to_string(&JsonValue::Object(object)));
        } else {
            println!("{}", output.summary);
        }
        return Ok(());
    }
    run_shell_supervisor_daemon(&cwd, args.json)
}

#[cfg(unix)]
fn run_shell_supervisor_daemon(cwd: &Path, json: bool) -> AppResult<()> {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::os::unix::net::{UnixListener, UnixStream};

    let state_dir = cwd.join(".dscode/shell-supervisor");
    fs::create_dir_all(&state_dir)?;
    fs::set_permissions(&state_dir, fs::Permissions::from_mode(0o700))?;
    let socket = state_dir.join("supervisor.sock");
    if let Some(path_bytes) = unix_socket_path_too_long(&socket) {
        return Err(app_error(format!(
            "shell supervisor socket path is too long for Unix domain sockets: {} ({} bytes; limit is < {} bytes). Use a shorter workspace path.",
            socket.display(),
            path_bytes,
            SHELL_SUPERVISOR_UNIX_SOCKET_MAX_BYTES
        )));
    }
    if socket.exists() {
        if UnixStream::connect(&socket).is_ok() {
            return Err(app_error(format!(
                "shell supervisor socket is already active: {}",
                socket.display()
            )));
        }
        fs::remove_file(&socket)?;
    }
    let listener = UnixListener::bind(&socket)?;
    fs::set_permissions(&socket, fs::Permissions::from_mode(0o600))?;
    let epoch = format_epoch_seconds(current_epoch_seconds());
    write_shell_supervisor_manifest(cwd, &socket, &epoch)?;
    if json {
        println!(
            "{}",
            json_value_to_string(&shell_supervisor_event_json(
                "started", cwd, &socket, &epoch, None,
            ))
        );
    } else {
        println!(
            "shell supervisor protocol bridge listening: {}",
            socket.display()
        );
    }

    for stream in listener.incoming() {
        let stream = stream?;
        let shutdown = handle_shell_supervisor_stream(stream, cwd, &socket, &epoch)?;
        if shutdown {
            break;
        }
    }
    let _ = fs::remove_file(&socket);
    if json {
        println!(
            "{}",
            json_value_to_string(&shell_supervisor_event_json(
                "stopped", cwd, &socket, &epoch, None,
            ))
        );
    }
    Ok(())
}

#[cfg(unix)]
fn handle_shell_supervisor_stream(
    mut stream: std::os::unix::net::UnixStream,
    cwd: &Path,
    socket: &Path,
    epoch: &str,
) -> AppResult<bool> {
    let line = read_shell_supervisor_request_line(&mut stream)?;
    let (response, shutdown) = match parse_shell_supervisor_request(&line) {
        Ok(request) if request.method == "attach_stream" => {
            shell_supervisor_stream_attach(&mut stream, &request, cwd, socket, epoch)?;
            return Ok(false);
        }
        Ok(request) if request.method == "byte_stream" => {
            shell_supervisor_stream_byte(&mut stream, &request, cwd, socket, epoch)?;
            return Ok(false);
        }
        Ok(request) if request.method == "pty_fd" => {
            shell_supervisor_stream_pty_fd(&mut stream, &request, cwd, socket, epoch)?;
            return Ok(false);
        }
        Ok(request) => (
            shell_supervisor_protocol_response_for_request(&request, cwd, socket, epoch),
            request.method == "shutdown",
        ),
        Err(error) => (
            shell_supervisor_protocol_error_response(cwd, socket, epoch, &error.to_string()),
            false,
        ),
    };
    stream.write_all(json_value_to_string(&response).as_bytes())?;
    stream.write_all(b"\n")?;
    stream.flush()?;
    Ok(shutdown)
}

#[cfg(windows)]
fn run_shell_supervisor_daemon(cwd: &Path, json: bool) -> AppResult<()> {
    use std::fs;
    use std::net::TcpListener;

    let state_dir = cwd.join(".dscode/shell-supervisor");
    fs::create_dir_all(&state_dir)?;
    let endpoint_file = shell_supervisor_tcp_endpoint_path(cwd);
    if endpoint_file.exists() {
        match read_shell_supervisor_tcp_endpoint(cwd) {
            Ok(endpoint) if shell_supervisor_tcp_endpoint_is_active(&endpoint) => {
                return Err(app_error(format!(
                    "shell supervisor tcp endpoint is already active: tcp://{endpoint}"
                )));
            }
            _ => {
                fs::remove_file(&endpoint_file)?;
            }
        }
    }

    let listener = TcpListener::bind("127.0.0.1:0")?;
    let endpoint = format!("tcp://{}", listener.local_addr()?);
    fs::write(&endpoint_file, format!("{endpoint}\n"))?;
    let epoch = format_epoch_seconds(current_epoch_seconds());
    let active_jobs = count_active_durable_shell_jobs(cwd)?;
    write_shell_supervisor_manifest_snapshot(
        cwd,
        Path::new(&endpoint),
        &epoch,
        &epoch,
        active_jobs,
    )?;
    if json {
        println!(
            "{}",
            json_value_to_string(&shell_supervisor_event_json(
                "started",
                cwd,
                Path::new(&endpoint),
                &epoch,
                None,
            ))
        );
    } else {
        println!("shell supervisor protocol bridge listening: {endpoint}");
    }

    for stream in listener.incoming() {
        let stream = stream?;
        let shutdown =
            handle_shell_supervisor_tcp_stream(stream, cwd, Path::new(&endpoint), &epoch)?;
        if shutdown {
            break;
        }
    }
    let _ = fs::remove_file(&endpoint_file);
    if json {
        println!(
            "{}",
            json_value_to_string(&shell_supervisor_event_json(
                "stopped",
                cwd,
                Path::new(&endpoint),
                &epoch,
                None,
            ))
        );
    }
    Ok(())
}

#[cfg(windows)]
fn handle_shell_supervisor_tcp_stream(
    mut stream: std::net::TcpStream,
    cwd: &Path,
    endpoint: &Path,
    epoch: &str,
) -> AppResult<bool> {
    let line = read_shell_supervisor_request_line(&mut stream)?;
    let (response, shutdown) = match parse_shell_supervisor_request(&line) {
        Ok(request) if request.method == "attach_stream" => {
            shell_supervisor_stream_attach(&mut stream, &request, cwd, endpoint, epoch)?;
            return Ok(false);
        }
        Ok(request) if request.method == "byte_stream" => (
            shell_supervisor_stream_unsupported_response(
                cwd,
                endpoint,
                epoch,
                "byte_stream",
                "pty_byte_stream",
                "shell supervisor byte_stream currently requires the Unix socket streaming path",
            ),
            false,
        ),
        Ok(request) if request.method == "pty_fd" => (
            shell_supervisor_stream_unsupported_response(
                cwd,
                endpoint,
                epoch,
                "pty_fd",
                "pty_fd_handoff",
                "shell supervisor pty_fd handoff requires Linux SCM_RIGHTS support",
            ),
            false,
        ),
        Ok(request) => (
            shell_supervisor_protocol_response_for_request(&request, cwd, endpoint, epoch),
            request.method == "shutdown",
        ),
        Err(error) => (
            shell_supervisor_protocol_error_response(cwd, endpoint, epoch, &error.to_string()),
            false,
        ),
    };
    stream.write_all(json_value_to_string(&response).as_bytes())?;
    stream.write_all(b"\n")?;
    stream.flush()?;
    Ok(shutdown)
}

#[cfg(not(any(unix, windows)))]
fn run_shell_supervisor_daemon(_cwd: &Path, _json: bool) -> AppResult<()> {
    Err(app_error(
        "shell supervisor protocol bridge is currently supported only on Unix and Windows",
    ))
}

fn read_shell_supervisor_request_line<R: Read>(stream: &mut R) -> AppResult<String> {
    const MAX_REQUEST_LINE_BYTES: usize = 1024 * 1024;
    let mut line = Vec::new();
    let mut byte = [0u8; 1];
    loop {
        let read = stream.read(&mut byte)?;
        if read == 0 || byte[0] == b'\n' {
            break;
        }
        line.push(byte[0]);
        if line.len() > MAX_REQUEST_LINE_BYTES {
            return Err(app_error("shell supervisor request line is too large"));
        }
    }
    String::from_utf8(line)
        .map_err(|_| app_error("shell supervisor request line must be UTF-8 JSON"))
}

#[cfg(all(unix, target_os = "linux"))]
fn shell_supervisor_stream_pty_fd(
    stream: &mut std::os::unix::net::UnixStream,
    request: &ShellSupervisorRequest,
    cwd: &Path,
    socket: &Path,
    epoch: &str,
) -> AppResult<()> {
    let result: AppResult<()> = (|| {
        let task_id =
            shell_supervisor_request_task_id(request, "shell supervisor pty_fd requires task_id")?;
        if shell_supervisor_request_value(request, "tty_rows").is_some()
            || shell_supervisor_request_value(request, "rows").is_some()
            || shell_supervisor_request_value(request, "tty_cols").is_some()
            || shell_supervisor_request_value(request, "cols").is_some()
        {
            let _ = shell_supervisor_resize_job(request, cwd)?;
        }
        let lease = lease_native_supervisor_pty_master_fd(&task_id)?;
        let response = JsonValue::Object(BTreeMap::from([
            (
                "kind".to_string(),
                JsonValue::String("deepseek.exec_shell.supervisor.response.v1".to_string()),
            ),
            (
                "method".to_string(),
                JsonValue::String("pty_fd".to_string()),
            ),
            (
                "stream_method".to_string(),
                JsonValue::String("pty_fd_handoff".to_string()),
            ),
            ("status".to_string(), JsonValue::String("ok".to_string())),
            ("task_id".to_string(), JsonValue::String(task_id.clone())),
            (
                "cwd".to_string(),
                JsonValue::String(cwd.display().to_string()),
            ),
            (
                "supervisor_pid".to_string(),
                JsonValue::Number(std::process::id().to_string()),
            ),
            (
                "supervisor_socket".to_string(),
                JsonValue::String(socket.display().to_string()),
            ),
            (
                "supervisor_epoch".to_string(),
                JsonValue::String(epoch.to_string()),
            ),
            (
                "handoff".to_string(),
                JsonValue::String("scm_rights".to_string()),
            ),
            ("exclusive_reader_pause".to_string(), JsonValue::Bool(true)),
        ]));
        stream.write_all(json_value_to_string(&response).as_bytes())?;
        stream.write_all(b"\n")?;
        stream.flush()?;
        send_shell_supervisor_fd(stream, lease.fd())?;
        let max_ms = shell_supervisor_request_u64(request, "max_ms")
            .unwrap_or(300_000)
            .clamp(100, 300_000);
        wait_for_shell_supervisor_fd_handoff_close(stream, Duration::from_millis(max_ms))?;
        drop(lease);
        Ok(())
    })();

    if let Err(error) = result {
        let mut response =
            shell_supervisor_protocol_error_response(cwd, socket, epoch, &error.to_string());
        if let JsonValue::Object(root) = &mut response {
            root.insert(
                "method".to_string(),
                JsonValue::String("pty_fd".to_string()),
            );
            root.insert(
                "stream_method".to_string(),
                JsonValue::String("pty_fd_handoff".to_string()),
            );
        }
        stream.write_all(json_value_to_string(&response).as_bytes())?;
        stream.write_all(b"\n")?;
        stream.flush()?;
    }
    Ok(())
}

#[cfg(all(unix, not(target_os = "linux")))]
fn shell_supervisor_stream_pty_fd(
    stream: &mut std::os::unix::net::UnixStream,
    _request: &ShellSupervisorRequest,
    cwd: &Path,
    socket: &Path,
    epoch: &str,
) -> AppResult<()> {
    let mut response = shell_supervisor_protocol_error_response(
        cwd,
        socket,
        epoch,
        "shell supervisor pty_fd handoff requires Linux SCM_RIGHTS support",
    );
    if let JsonValue::Object(root) = &mut response {
        root.insert(
            "method".to_string(),
            JsonValue::String("pty_fd".to_string()),
        );
        root.insert(
            "stream_method".to_string(),
            JsonValue::String("pty_fd_handoff".to_string()),
        );
    }
    stream.write_all(json_value_to_string(&response).as_bytes())?;
    stream.write_all(b"\n")?;
    stream.flush()?;
    Ok(())
}

#[cfg(all(unix, target_os = "linux"))]
fn wait_for_shell_supervisor_fd_handoff_close(
    stream: &mut std::os::unix::net::UnixStream,
    timeout: Duration,
) -> AppResult<()> {
    stream.set_read_timeout(Some(timeout))?;
    let mut buffer = [0u8; 1024];
    loop {
        match stream.read(&mut buffer) {
            Ok(0) => break,
            Ok(_) => {}
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
                ) =>
            {
                break;
            }
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

fn shell_supervisor_stream_attach<W: Write>(
    stream: &mut W,
    request: &ShellSupervisorRequest,
    cwd: &Path,
    socket: &Path,
    epoch: &str,
) -> AppResult<()> {
    const DEFAULT_STREAM_MAX_MS: u64 = 30_000;
    const MAX_STREAM_MAX_MS: u64 = 300_000;
    const DEFAULT_STREAM_POLL_MS: u64 = 100;
    const MAX_STREAM_EVENTS: u64 = 5_000;

    let max_ms = shell_supervisor_request_u64(request, "max_ms")
        .unwrap_or(DEFAULT_STREAM_MAX_MS)
        .min(MAX_STREAM_MAX_MS);
    let poll_ms = shell_supervisor_request_u64(request, "poll_ms")
        .unwrap_or(DEFAULT_STREAM_POLL_MS)
        .clamp(10, 5_000);
    let max_events = shell_supervisor_request_u64(request, "max_events")
        .unwrap_or(MAX_STREAM_EVENTS)
        .clamp(1, MAX_STREAM_EVENTS);
    let mut cursor = shell_supervisor_request_u64(request, "cursor")
        .or_else(|| shell_supervisor_request_u64(request, "since_seq"))
        .or_else(|| shell_supervisor_request_u64(request, "sinceSeq"))
        .unwrap_or(0);
    let started = Instant::now();
    let deadline = started + Duration::from_millis(max_ms);
    let mut emitted = 0u64;
    let mut first = true;

    loop {
        let remaining_ms = deadline
            .checked_duration_since(Instant::now())
            .map(|duration| duration.as_millis().min(u128::from(poll_ms)) as u64)
            .unwrap_or(0);
        let attach_request =
            shell_supervisor_attach_stream_frame_request(request, cursor, remaining_ms, first);
        let mut response =
            shell_supervisor_protocol_response_for_request(&attach_request, cwd, socket, epoch);
        let (status, next_cursor, event_count, has_raw_outputs) =
            shell_supervisor_attach_stream_frame_state(&response);
        let response_status = json_as_object(&response)
            .and_then(|root| root.get("status"))
            .and_then(json_as_string)
            .unwrap_or("unknown")
            .to_string();
        let stream_done = response_status == "error"
            || response_status == "unsupported"
            || status != "running"
            || emitted + 1 >= max_events
            || Instant::now() >= deadline;
        let should_emit = first
            || event_count > 0
            || has_raw_outputs
            || stream_done
            || response_status == "error"
            || response_status == "unsupported";
        if should_emit {
            if let JsonValue::Object(root) = &mut response {
                root.insert(
                    "method".to_string(),
                    JsonValue::String("attach_stream".to_string()),
                );
                root.insert(
                    "stream_method".to_string(),
                    JsonValue::String("attach".to_string()),
                );
                root.insert(
                    "frame_index".to_string(),
                    JsonValue::Number((emitted + 1).to_string()),
                );
                root.insert("stream_done".to_string(), JsonValue::Bool(stream_done));
                root.insert(
                    "stream_cursor".to_string(),
                    JsonValue::Number(cursor.to_string()),
                );
                root.insert(
                    "stream_next_cursor".to_string(),
                    JsonValue::Number(next_cursor.to_string()),
                );
            }
            stream.write_all(json_value_to_string(&response).as_bytes())?;
            stream.write_all(b"\n")?;
            stream.flush()?;
            emitted = emitted.saturating_add(1);
        }
        if stream_done {
            break;
        }
        if next_cursor > cursor {
            cursor = next_cursor;
        } else {
            std::thread::sleep(Duration::from_millis(poll_ms));
        }
        first = false;
    }
    Ok(())
}

#[cfg(unix)]
fn shell_supervisor_stream_byte(
    stream: &mut std::os::unix::net::UnixStream,
    request: &ShellSupervisorRequest,
    cwd: &Path,
    socket: &Path,
    epoch: &str,
) -> AppResult<()> {
    const DEFAULT_STREAM_MAX_MS: u64 = 30_000;
    const MAX_STREAM_MAX_MS: u64 = 300_000;
    const DEFAULT_STREAM_POLL_MS: u64 = 25;
    const MAX_STREAM_EVENTS: u64 = 5_000;

    let control = match shell_supervisor_byte_stream_control_json(request, cwd) {
        Ok(control) => control,
        Err(error) => {
            let mut response =
                shell_supervisor_protocol_error_response(cwd, socket, epoch, &error.to_string());
            if let JsonValue::Object(root) = &mut response {
                root.insert(
                    "method".to_string(),
                    JsonValue::String("byte_stream".to_string()),
                );
                root.insert(
                    "stream_method".to_string(),
                    JsonValue::String("pty_byte_stream".to_string()),
                );
                root.insert("stream_done".to_string(), JsonValue::Bool(true));
            }
            stream.write_all(json_value_to_string(&response).as_bytes())?;
            stream.write_all(b"\n")?;
            stream.flush()?;
            return Ok(());
        }
    };
    if shell_supervisor_request_bool(request, "raw_proxy") {
        return shell_supervisor_stream_byte_raw_proxy(stream, request, cwd);
    }

    let max_ms = shell_supervisor_request_u64(request, "max_ms")
        .unwrap_or(DEFAULT_STREAM_MAX_MS)
        .min(MAX_STREAM_MAX_MS);
    let poll_ms = shell_supervisor_request_u64(request, "poll_ms")
        .unwrap_or(DEFAULT_STREAM_POLL_MS)
        .clamp(10, 5_000);
    let max_events = shell_supervisor_request_u64(request, "max_events")
        .unwrap_or(MAX_STREAM_EVENTS)
        .clamp(1, MAX_STREAM_EVENTS);
    let mut cursor = shell_supervisor_request_u64(request, "cursor")
        .or_else(|| shell_supervisor_request_u64(request, "since_seq"))
        .or_else(|| shell_supervisor_request_u64(request, "sinceSeq"))
        .unwrap_or(0);
    let started = Instant::now();
    let deadline = started + Duration::from_millis(max_ms);
    let mut emitted = 0u64;
    let mut first = true;
    let mut incoming = Vec::new();
    let mut detach_requested = false;

    loop {
        let control_frames =
            shell_supervisor_byte_stream_drain_control_frames(stream, request, cwd, &mut incoming)?;
        if control_frames
            .iter()
            .any(shell_supervisor_byte_stream_control_frame_detaches)
        {
            detach_requested = true;
        }
        let remaining_ms = deadline
            .checked_duration_since(Instant::now())
            .map(|duration| duration.as_millis().min(u128::from(poll_ms)) as u64)
            .unwrap_or(0);
        let attach_request =
            shell_supervisor_attach_stream_frame_request(request, cursor, remaining_ms, first);
        let mut response =
            shell_supervisor_protocol_response_for_request(&attach_request, cwd, socket, epoch);
        let (status, next_cursor, event_count, has_raw_outputs) =
            shell_supervisor_attach_stream_frame_state(&response);
        let byte_outputs = shell_supervisor_byte_outputs_json_from_response(&response);
        let response_status = json_as_object(&response)
            .and_then(|root| root.get("status"))
            .and_then(json_as_string)
            .unwrap_or("unknown")
            .to_string();
        let stream_done = response_status == "error"
            || response_status == "unsupported"
            || status != "running"
            || detach_requested
            || emitted + 1 >= max_events
            || Instant::now() >= deadline;
        let should_emit = first
            || event_count > 0
            || has_raw_outputs
            || byte_outputs.is_some()
            || !control_frames.is_empty()
            || stream_done
            || response_status == "error"
            || response_status == "unsupported";
        if should_emit {
            if let JsonValue::Object(root) = &mut response {
                root.insert(
                    "method".to_string(),
                    JsonValue::String("byte_stream".to_string()),
                );
                root.insert(
                    "stream_method".to_string(),
                    JsonValue::String("pty_byte_stream".to_string()),
                );
                root.insert(
                    "frame_index".to_string(),
                    JsonValue::Number((emitted + 1).to_string()),
                );
                root.insert("stream_done".to_string(), JsonValue::Bool(stream_done));
                root.insert(
                    "stream_cursor".to_string(),
                    JsonValue::Number(cursor.to_string()),
                );
                root.insert(
                    "stream_next_cursor".to_string(),
                    JsonValue::Number(next_cursor.to_string()),
                );
                if first {
                    if let Some(control) = control.clone() {
                        root.insert("control".to_string(), control);
                    }
                }
                if let Some(outputs) = byte_outputs {
                    root.insert("byte_outputs".to_string(), outputs);
                }
                if !control_frames.is_empty() {
                    root.insert(
                        "control_frames".to_string(),
                        JsonValue::Array(control_frames.clone()),
                    );
                }
            }
            stream.write_all(json_value_to_string(&response).as_bytes())?;
            stream.write_all(b"\n")?;
            stream.flush()?;
            emitted = emitted.saturating_add(1);
        }
        if stream_done {
            break;
        }
        if next_cursor > cursor {
            cursor = next_cursor;
        } else {
            std::thread::sleep(Duration::from_millis(poll_ms));
        }
        first = false;
    }
    Ok(())
}

#[cfg(unix)]
fn shell_supervisor_stream_byte_raw_proxy(
    stream: &mut std::os::unix::net::UnixStream,
    request: &ShellSupervisorRequest,
    cwd: &Path,
) -> AppResult<()> {
    const DEFAULT_STREAM_MAX_MS: u64 = 30_000;
    const MAX_STREAM_MAX_MS: u64 = 300_000;
    const DEFAULT_STREAM_POLL_MS: u64 = 10;

    let max_ms = shell_supervisor_request_u64(request, "max_ms")
        .unwrap_or(DEFAULT_STREAM_MAX_MS)
        .min(MAX_STREAM_MAX_MS);
    let poll_ms = shell_supervisor_request_u64(request, "poll_ms")
        .unwrap_or(DEFAULT_STREAM_POLL_MS)
        .clamp(5, 1_000);
    let limit_bytes = shell_supervisor_request_u64(request, "limit_bytes").or(Some(16 * 1024));
    let mut cursor = shell_supervisor_request_u64(request, "cursor")
        .or_else(|| shell_supervisor_request_u64(request, "since_seq"))
        .or_else(|| shell_supervisor_request_u64(request, "sinceSeq"))
        .unwrap_or(0);
    let deadline = Instant::now() + Duration::from_millis(max_ms);
    let mut first = true;
    let mut input_closed = false;

    loop {
        if !input_closed {
            match shell_supervisor_byte_stream_read_raw_proxy_input(stream)? {
                RawProxyInput::Bytes(bytes) => {
                    if !bytes.is_empty() {
                        let input_request =
                            shell_supervisor_byte_stream_raw_stdin_request(request, &bytes)?;
                        let _ = shell_supervisor_stdin_job(&input_request, cwd)?;
                    }
                }
                RawProxyInput::Closed => input_closed = true,
                RawProxyInput::None => {}
            }
        }

        let remaining_ms = deadline
            .checked_duration_since(Instant::now())
            .map(|duration| duration.as_millis().min(u128::from(poll_ms)) as u64)
            .unwrap_or(0);
        let mut attach_request =
            shell_supervisor_attach_stream_frame_request(request, cursor, remaining_ms, first);
        if let Some(limit_bytes) = limit_bytes {
            attach_request.args.insert(
                "limit_bytes".to_string(),
                JsonValue::Number(limit_bytes.to_string()),
            );
        }
        let response = shell_supervisor_protocol_response_for_request(
            &attach_request,
            cwd,
            Path::new("raw-proxy"),
            "raw-proxy",
        );
        let (status, next_cursor, event_count, has_raw_outputs) =
            shell_supervisor_attach_stream_frame_state(&response);
        let response_status = json_as_object(&response)
            .and_then(|root| root.get("status"))
            .and_then(json_as_string)
            .unwrap_or("unknown");
        let wrote = shell_supervisor_byte_stream_write_raw_outputs(stream, &response)?;
        let done = response_status == "error"
            || response_status == "unsupported"
            || status != "running"
            || Instant::now() >= deadline;
        if done {
            break;
        }
        if next_cursor > cursor {
            cursor = next_cursor;
        } else if !wrote && event_count == 0 && !has_raw_outputs {
            std::thread::sleep(Duration::from_millis(poll_ms));
        }
        first = false;
    }
    Ok(())
}

#[cfg(unix)]
enum RawProxyInput {
    Bytes(Vec<u8>),
    Closed,
    None,
}

#[cfg(unix)]
fn shell_supervisor_byte_stream_read_raw_proxy_input(
    stream: &mut std::os::unix::net::UnixStream,
) -> AppResult<RawProxyInput> {
    let mut buffer = [0u8; 4096];
    stream.set_nonblocking(true)?;
    let result = match stream.read(&mut buffer) {
        Ok(0) => Ok(RawProxyInput::Closed),
        Ok(count) => Ok(RawProxyInput::Bytes(buffer[..count].to_vec())),
        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => Ok(RawProxyInput::None),
        Err(error) if error.kind() == std::io::ErrorKind::Interrupted => Ok(RawProxyInput::None),
        Err(error) => Err(error.into()),
    };
    stream.set_nonblocking(false)?;
    result
}

#[cfg(unix)]
fn shell_supervisor_byte_stream_raw_stdin_request(
    request: &ShellSupervisorRequest,
    bytes: &[u8],
) -> AppResult<ShellSupervisorRequest> {
    let mut args = shell_supervisor_byte_stream_base_control_args(request)?;
    args.insert(
        "input_base64".to_string(),
        JsonValue::String(encode_shell_base64(bytes)),
    );
    args.insert("timeout_ms".to_string(), JsonValue::Number("0".to_string()));
    Ok(ShellSupervisorRequest {
        method: "stdin".to_string(),
        args,
    })
}

#[cfg(unix)]
fn shell_supervisor_byte_stream_write_raw_outputs(
    stream: &mut std::os::unix::net::UnixStream,
    response: &JsonValue,
) -> AppResult<bool> {
    let Some(outputs) = json_as_object(response)
        .and_then(|root| root.get("terminal_raw_outputs"))
        .and_then(json_as_array)
    else {
        return Ok(false);
    };
    let mut wrote = false;
    for output in outputs {
        let Some(object) = json_as_object(output) else {
            continue;
        };
        let Some(encoded) = object
            .get("raw_base64")
            .or_else(|| object.get("rawBase64"))
            .and_then(json_as_string)
        else {
            continue;
        };
        let bytes = decode_shell_base64(encoded)?;
        if !bytes.is_empty() {
            stream.write_all(&bytes)?;
            wrote = true;
        }
    }
    if wrote {
        stream.flush()?;
    }
    Ok(wrote)
}

fn shell_supervisor_attach_stream_frame_request(
    request: &ShellSupervisorRequest,
    cursor: u64,
    wait_ms: u64,
    first: bool,
) -> ShellSupervisorRequest {
    let mut args = request.args.clone();
    args.insert(
        "method".to_string(),
        JsonValue::String("attach".to_string()),
    );
    args.insert("cursor".to_string(), JsonValue::Number(cursor.to_string()));
    args.insert(
        "wait_ms".to_string(),
        JsonValue::Number(wait_ms.to_string()),
    );
    if !first {
        args.insert("tail".to_string(), JsonValue::Bool(false));
    }
    ShellSupervisorRequest {
        method: "attach".to_string(),
        args,
    }
}

fn shell_supervisor_attach_stream_frame_state(response: &JsonValue) -> (String, u64, u64, bool) {
    let Some(root) = json_as_object(response) else {
        return ("unknown".to_string(), 0, 0, false);
    };
    let summary = root
        .get("attach_summary")
        .and_then(json_as_string)
        .unwrap_or("");
    let status = shell_summary_value(summary, "status")
        .unwrap_or("unknown")
        .to_string();
    let next_cursor = shell_attach_summary_next_cursor(summary).unwrap_or(0);
    let event_count = shell_summary_value(summary, "events")
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(0);
    let has_raw_outputs = root
        .get("terminal_raw_outputs")
        .and_then(json_as_array)
        .is_some_and(|items| !items.is_empty());
    (status, next_cursor, event_count, has_raw_outputs)
}

#[cfg(unix)]
fn shell_supervisor_byte_stream_control_json(
    request: &ShellSupervisorRequest,
    supervisor_cwd: &Path,
) -> AppResult<Option<JsonValue>> {
    let mut control = BTreeMap::new();
    let has_rows = shell_supervisor_request_value(request, "tty_rows").is_some()
        || shell_supervisor_request_value(request, "rows").is_some();
    let has_cols = shell_supervisor_request_value(request, "tty_cols").is_some()
        || shell_supervisor_request_value(request, "cols").is_some();
    if has_rows || has_cols {
        if has_rows != has_cols {
            return Err(app_error(
                "shell supervisor byte_stream resize requires both rows and cols",
            ));
        }
        let output = shell_supervisor_resize_job(request, supervisor_cwd)?;
        control.insert(
            "resize_summary".to_string(),
            JsonValue::String(output.summary),
        );
    }

    let has_input = ["input", "stdin", "data"]
        .iter()
        .any(|key| shell_supervisor_request_value(request, key).is_some());
    if has_input || shell_supervisor_request_bool(request, "close_stdin") {
        let output = shell_supervisor_stdin_job(request, supervisor_cwd)?;
        control.insert(
            "stdin_summary".to_string(),
            JsonValue::String(output.summary),
        );
    }

    if control.is_empty() {
        Ok(None)
    } else {
        Ok(Some(JsonValue::Object(control)))
    }
}

#[cfg(unix)]
fn shell_supervisor_byte_stream_drain_control_frames(
    stream: &mut std::os::unix::net::UnixStream,
    request: &ShellSupervisorRequest,
    supervisor_cwd: &Path,
    incoming: &mut Vec<u8>,
) -> AppResult<Vec<JsonValue>> {
    const MAX_FRAME_BYTES: usize = 1024 * 1024;
    let mut buffer = [0u8; 4096];
    stream.set_nonblocking(true)?;
    loop {
        match stream.read(&mut buffer) {
            Ok(0) => break,
            Ok(count) => {
                incoming.extend_from_slice(&buffer[..count]);
                if incoming.len() > MAX_FRAME_BYTES {
                    stream.set_nonblocking(false)?;
                    return Err(app_error("shell supervisor byte_stream frame is too large"));
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => {
                stream.set_nonblocking(false)?;
                return Err(error.into());
            }
        }
    }
    stream.set_nonblocking(false)?;

    let mut frames = Vec::new();
    while let Some(index) = incoming.iter().position(|byte| *byte == b'\n') {
        let line = incoming.drain(..=index).collect::<Vec<_>>();
        let line = String::from_utf8(line)
            .map_err(|_| app_error("shell supervisor byte_stream frame must be UTF-8 JSON"))?;
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        frames.push(shell_supervisor_byte_stream_apply_control_frame(
            request,
            supervisor_cwd,
            line,
        )?);
    }
    Ok(frames)
}

#[cfg(unix)]
fn shell_supervisor_byte_stream_apply_control_frame(
    request: &ShellSupervisorRequest,
    supervisor_cwd: &Path,
    line: &str,
) -> AppResult<JsonValue> {
    let value = parse_json_value(line)?;
    let object = json_as_object(&value)
        .ok_or_else(|| app_error("shell supervisor byte_stream control frame must be an object"))?;
    let frame_type = object
        .get("type")
        .or_else(|| object.get("kind"))
        .or_else(|| object.get("method"))
        .and_then(json_as_string)
        .unwrap_or_else(|| {
            if object.get("tty_rows").is_some()
                || object.get("rows").is_some()
                || object.get("tty_cols").is_some()
                || object.get("cols").is_some()
            {
                "resize"
            } else {
                "stdin"
            }
        });
    match frame_type {
        "stdin" | "input" | "data" | "close_stdin" => {
            shell_supervisor_byte_stream_apply_stdin_frame(request, supervisor_cwd, object)
        }
        "resize" | "winsize" => {
            shell_supervisor_byte_stream_apply_resize_frame(request, supervisor_cwd, object)
        }
        "detach" | "close" => Ok(JsonValue::Object(BTreeMap::from([
            ("type".to_string(), JsonValue::String("detach".to_string())),
            ("status".to_string(), JsonValue::String("ok".to_string())),
        ]))),
        other => Err(app_error(format!(
            "unsupported shell supervisor byte_stream control frame `{other}`"
        ))),
    }
}

#[cfg(unix)]
fn shell_supervisor_byte_stream_apply_stdin_frame(
    request: &ShellSupervisorRequest,
    supervisor_cwd: &Path,
    frame: &BTreeMap<String, JsonValue>,
) -> AppResult<JsonValue> {
    let mut args = shell_supervisor_byte_stream_base_control_args(request)?;
    if let Some(input) = frame
        .get("input")
        .or_else(|| frame.get("stdin"))
        .or_else(|| frame.get("data"))
        .and_then(json_as_string)
    {
        args.insert("input".to_string(), JsonValue::String(input.to_string()));
    } else if let Some(encoded) = frame
        .get("bytes_base64")
        .or_else(|| frame.get("input_base64"))
        .or_else(|| frame.get("raw_base64"))
        .or_else(|| frame.get("rawBase64"))
        .and_then(json_as_string)
    {
        let bytes = decode_shell_base64(encoded)?;
        let input = String::from_utf8(bytes).map_err(|_| {
            app_error("shell supervisor byte_stream stdin bytes_base64 must decode to UTF-8")
        })?;
        args.insert("input".to_string(), JsonValue::String(input));
    }
    if frame
        .get("close_stdin")
        .or_else(|| frame.get("closeStdin"))
        .is_some_and(|value| shell_json_truthy(value))
        || frame
            .get("type")
            .and_then(json_as_string)
            .is_some_and(|value| value == "close_stdin")
    {
        args.insert("close_stdin".to_string(), JsonValue::Bool(true));
    }
    if let Some(timeout_ms) = frame.get("timeout_ms").and_then(json_as_u64) {
        args.insert(
            "timeout_ms".to_string(),
            JsonValue::Number(timeout_ms.to_string()),
        );
    }
    if !args.contains_key("input") && !args.contains_key("close_stdin") {
        return Err(app_error(
            "shell supervisor byte_stream stdin frame requires input, bytes_base64, or close_stdin",
        ));
    }
    let frame_request = ShellSupervisorRequest {
        method: "stdin".to_string(),
        args,
    };
    let output = shell_supervisor_stdin_job(&frame_request, supervisor_cwd)?;
    Ok(JsonValue::Object(BTreeMap::from([
        ("type".to_string(), JsonValue::String("stdin".to_string())),
        ("status".to_string(), JsonValue::String("ok".to_string())),
        (
            "stdin_summary".to_string(),
            JsonValue::String(output.summary),
        ),
    ])))
}

#[cfg(unix)]
fn shell_supervisor_byte_stream_apply_resize_frame(
    request: &ShellSupervisorRequest,
    supervisor_cwd: &Path,
    frame: &BTreeMap<String, JsonValue>,
) -> AppResult<JsonValue> {
    let rows = frame
        .get("tty_rows")
        .or_else(|| frame.get("rows"))
        .and_then(json_as_u64)
        .ok_or_else(|| app_error("shell supervisor byte_stream resize frame requires rows"))?;
    let cols = frame
        .get("tty_cols")
        .or_else(|| frame.get("cols"))
        .and_then(json_as_u64)
        .ok_or_else(|| app_error("shell supervisor byte_stream resize frame requires cols"))?;
    let mut args = shell_supervisor_byte_stream_base_control_args(request)?;
    args.insert("tty_rows".to_string(), JsonValue::Number(rows.to_string()));
    args.insert("tty_cols".to_string(), JsonValue::Number(cols.to_string()));
    let frame_request = ShellSupervisorRequest {
        method: "resize".to_string(),
        args,
    };
    let output = shell_supervisor_resize_job(&frame_request, supervisor_cwd)?;
    Ok(JsonValue::Object(BTreeMap::from([
        ("type".to_string(), JsonValue::String("resize".to_string())),
        ("status".to_string(), JsonValue::String("ok".to_string())),
        (
            "resize_summary".to_string(),
            JsonValue::String(output.summary),
        ),
    ])))
}

#[cfg(unix)]
fn shell_supervisor_byte_stream_base_control_args(
    request: &ShellSupervisorRequest,
) -> AppResult<BTreeMap<String, JsonValue>> {
    let task_id = shell_supervisor_request_task_id(
        request,
        "shell supervisor byte_stream control frame requires task_id on the stream request",
    )?;
    let mut args = BTreeMap::from([("task_id".to_string(), JsonValue::String(task_id))]);
    if let Some(cwd) = shell_supervisor_request_scalar(request, "cwd")? {
        args.insert("cwd".to_string(), JsonValue::String(cwd));
    }
    Ok(args)
}

#[cfg(unix)]
fn shell_supervisor_byte_stream_control_frame_detaches(frame: &JsonValue) -> bool {
    json_as_object(frame)
        .and_then(|object| object.get("type"))
        .and_then(json_as_string)
        .is_some_and(|value| value == "detach")
}

#[cfg(unix)]
fn shell_json_truthy(value: &JsonValue) -> bool {
    match value {
        JsonValue::Bool(value) => *value,
        JsonValue::String(value) => matches!(value.trim(), "1" | "true" | "TRUE" | "yes" | "on"),
        JsonValue::Number(value) => value == "1",
        JsonValue::Null | JsonValue::Array(_) | JsonValue::Object(_) => false,
    }
}

#[cfg(unix)]
fn shell_supervisor_byte_outputs_json_from_response(response: &JsonValue) -> Option<JsonValue> {
    let root = json_as_object(response)?;
    let outputs = root.get("terminal_raw_outputs").and_then(json_as_array)?;
    let mut byte_outputs = Vec::new();
    for output in outputs {
        let Some(output) = json_as_object(output) else {
            continue;
        };
        let Some(encoded) = output
            .get("raw_base64")
            .or_else(|| output.get("rawBase64"))
            .and_then(json_as_string)
        else {
            continue;
        };
        let mut item = BTreeMap::from([
            ("kind".to_string(), JsonValue::String("output".to_string())),
            (
                "bytes_base64".to_string(),
                JsonValue::String(encoded.to_string()),
            ),
        ]);
        if let Some(seq) = output.get("seq").and_then(json_as_u64) {
            item.insert("seq".to_string(), JsonValue::Number(seq.to_string()));
        }
        byte_outputs.push(JsonValue::Object(item));
    }
    (!byte_outputs.is_empty()).then_some(JsonValue::Array(byte_outputs))
}

#[derive(Debug, Clone)]
struct ShellSupervisorRequest {
    method: String,
    args: BTreeMap<String, JsonValue>,
}

impl ShellSupervisorRequest {
    #[cfg(test)]
    fn method_only(method: &str) -> Self {
        Self {
            method: method.to_string(),
            args: BTreeMap::new(),
        }
    }
}

fn parse_shell_supervisor_request(line: &str) -> AppResult<ShellSupervisorRequest> {
    let value = if line.trim().is_empty() {
        JsonValue::Object(BTreeMap::new())
    } else {
        parse_json_value(line.trim())?
    };
    let Some(object) = json_as_object(&value) else {
        return Err(app_error("shell supervisor request must be a JSON object"));
    };
    Ok(ShellSupervisorRequest {
        method: object
            .get("method")
            .and_then(json_as_string)
            .unwrap_or("health")
            .to_string(),
        args: object.clone(),
    })
}

#[cfg(test)]
fn parse_shell_supervisor_method(line: &str) -> AppResult<String> {
    Ok(parse_shell_supervisor_request(line)?.method)
}

#[cfg(test)]
fn shell_supervisor_protocol_response(
    method: &str,
    cwd: &Path,
    socket: &Path,
    epoch: &str,
) -> JsonValue {
    shell_supervisor_protocol_response_for_request(
        &ShellSupervisorRequest::method_only(method),
        cwd,
        socket,
        epoch,
    )
}

fn shell_supervisor_protocol_response_for_request(
    request: &ShellSupervisorRequest,
    cwd: &Path,
    socket: &Path,
    epoch: &str,
) -> JsonValue {
    let method = request.method.as_str();
    let supported = SHELL_SUPERVISOR_SUPPORTED_METHODS.contains(&method);
    let (mut active_jobs, mut active_jobs_error) = match count_active_durable_shell_jobs(cwd) {
        Ok(count) => (count, None),
        Err(error) => (0, Some(error.to_string())),
    };
    let mut response = BTreeMap::from([
        (
            "kind".to_string(),
            JsonValue::String("deepseek.exec_shell.supervisor.response.v1".to_string()),
        ),
        ("method".to_string(), JsonValue::String(method.to_string())),
        (
            "status".to_string(),
            JsonValue::String(if supported { "ok" } else { "unsupported" }.to_string()),
        ),
        (
            "cwd".to_string(),
            JsonValue::String(cwd.display().to_string()),
        ),
        (
            "supervisor_pid".to_string(),
            JsonValue::Number(std::process::id().to_string()),
        ),
        (
            "supervisor_socket".to_string(),
            JsonValue::String(socket.display().to_string()),
        ),
        (
            "supervisor_epoch".to_string(),
            JsonValue::String(epoch.to_string()),
        ),
        (
            "protocol".to_string(),
            JsonValue::String("newline-json-v1".to_string()),
        ),
        (
            "methods".to_string(),
            shell_supervisor_method_json(SHELL_SUPERVISOR_SUPPORTED_METHODS),
        ),
        (
            "unsupported_methods".to_string(),
            shell_supervisor_method_json(SHELL_SUPERVISOR_UNSUPPORTED_PTY_METHODS),
        ),
        (
            "pty_backend".to_string(),
            JsonValue::String("none".to_string()),
        ),
        (
            "native_pty".to_string(),
            JsonValue::Bool(native_supervisor_pty_supported()),
        ),
        (
            "active_jobs".to_string(),
            JsonValue::Number(active_jobs.to_string()),
        ),
    ]);
    if !supported {
        response.insert(
            "error".to_string(),
            JsonValue::String(format!(
                "shell supervisor method `{method}` is not supported by this protocol"
            )),
        );
    } else {
        match method {
            "show" => {
                let inventory = ExecShellListTool
                    .execute(ToolInput::new().with_arg("cwd", cwd.display().to_string()));
                match inventory {
                    Ok(output) => {
                        response.insert(
                            "job_inventory".to_string(),
                            JsonValue::String(output.summary),
                        );
                    }
                    Err(error) => {
                        response.insert(
                            "job_inventory_error".to_string(),
                            JsonValue::String(error.to_string()),
                        );
                    }
                }
            }
            "start" => match shell_supervisor_start_job(request, cwd, socket, epoch) {
                Ok(start) => {
                    response.insert("task_id".to_string(), JsonValue::String(start.task_id));
                    response.insert(
                        "start_summary".to_string(),
                        JsonValue::String(start.summary),
                    );
                    response.insert("job_tty".to_string(), JsonValue::Bool(start.tty));
                    response.insert(
                        "job_pty_backend".to_string(),
                        JsonValue::String(start.pty_backend),
                    );
                    match count_active_durable_shell_jobs(cwd) {
                        Ok(count) => {
                            active_jobs = count;
                            active_jobs_error = None;
                            response.insert(
                                "active_jobs".to_string(),
                                JsonValue::Number(active_jobs.to_string()),
                            );
                        }
                        Err(error) => {
                            active_jobs = 0;
                            active_jobs_error = Some(error.to_string());
                            response.insert(
                                "active_jobs".to_string(),
                                JsonValue::Number(active_jobs.to_string()),
                            );
                        }
                    }
                }
                Err(error) => {
                    response.insert("status".to_string(), JsonValue::String("error".to_string()));
                    response.insert("error".to_string(), JsonValue::String(error.to_string()));
                }
            },
            "wait" => shell_supervisor_apply_tool_result(
                &mut response,
                "wait_summary",
                shell_supervisor_wait_job(request, cwd),
            ),
            "replay" => shell_supervisor_apply_tool_result(
                &mut response,
                "replay_summary",
                shell_supervisor_replay_job(request, cwd),
            ),
            "attach" => shell_supervisor_apply_attach_result(
                &mut response,
                shell_supervisor_attach_job(request, cwd),
            ),
            "stdin" => shell_supervisor_apply_tool_result(
                &mut response,
                "stdin_summary",
                shell_supervisor_stdin_job(request, cwd),
            ),
            "resize" => shell_supervisor_apply_tool_result(
                &mut response,
                "resize_summary",
                shell_supervisor_resize_job(request, cwd),
            ),
            "cancel" => shell_supervisor_apply_tool_result(
                &mut response,
                "cancel_summary",
                shell_supervisor_cancel_job(request, cwd),
            ),
            _ => {}
        }
    }
    if supported && !matches!(method, "health" | "status" | "show" | "shutdown") {
        match count_active_durable_shell_jobs(cwd) {
            Ok(count) => {
                active_jobs = count;
                active_jobs_error = None;
                response.insert(
                    "active_jobs".to_string(),
                    JsonValue::Number(active_jobs.to_string()),
                );
            }
            Err(error) => {
                active_jobs = 0;
                active_jobs_error = Some(error.to_string());
                response.insert(
                    "active_jobs".to_string(),
                    JsonValue::Number(active_jobs.to_string()),
                );
            }
        }
    }
    if let Some(error) = active_jobs_error {
        response.insert("active_jobs_error".to_string(), JsonValue::String(error));
    } else if let Err(error) =
        refresh_shell_supervisor_manifest_if_present(cwd, socket, epoch, active_jobs)
    {
        response.insert(
            "manifest_refresh_error".to_string(),
            JsonValue::String(error.to_string()),
        );
    }
    JsonValue::Object(response)
}

fn shell_supervisor_apply_tool_result(
    response: &mut BTreeMap<String, JsonValue>,
    summary_key: &str,
    result: AppResult<crate::tools::types::ToolOutput>,
) {
    match result {
        Ok(output) => {
            response.insert(summary_key.to_string(), JsonValue::String(output.summary));
        }
        Err(error) => {
            response.insert("status".to_string(), JsonValue::String("error".to_string()));
            response.insert("error".to_string(), JsonValue::String(error.to_string()));
        }
    }
}

fn shell_supervisor_apply_attach_result(
    response: &mut BTreeMap<String, JsonValue>,
    result: AppResult<crate::tools::types::ToolOutput>,
) {
    match result {
        Ok(output) => {
            if let Some(raw_outputs) = shell_attach_raw_outputs_json_from_summary(&output.summary) {
                response.insert("terminal_raw_outputs".to_string(), raw_outputs);
            }
            response.insert(
                "attach_summary".to_string(),
                JsonValue::String(output.summary),
            );
        }
        Err(error) => {
            response.insert("status".to_string(), JsonValue::String("error".to_string()));
            response.insert("error".to_string(), JsonValue::String(error.to_string()));
        }
    }
}

struct ShellSupervisorStart {
    task_id: String,
    summary: String,
    tty: bool,
    pty_backend: String,
}

fn shell_supervisor_start_job(
    request: &ShellSupervisorRequest,
    supervisor_cwd: &Path,
    socket: &Path,
    epoch: &str,
) -> AppResult<ShellSupervisorStart> {
    let command = shell_supervisor_request_string(request, "command")
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| app_error("shell supervisor start requires command"))?;
    let cwd = shell_supervisor_request_cwd(request, supervisor_cwd)?;
    let mut input = ToolInput::new()
        .with_arg("command", command.to_string())
        .with_arg("cwd", cwd.display().to_string());
    for key in [
        "stdin",
        "input",
        "timeout_ms",
        "tty",
        "tty_rows",
        "tty_cols",
    ] {
        if let Some(value) = shell_supervisor_request_scalar(request, key)? {
            input = input.with_arg(key, value);
        }
    }
    if let Some(env) = shell_supervisor_request_object(request, "env") {
        for (key, value) in env {
            let Some(value) = shell_supervisor_json_scalar(value) else {
                return Err(app_error(format!(
                    "shell supervisor start env.{key} must be a string, number, or bool"
                )));
            };
            input = input.with_arg(format!("env.{key}"), value);
        }
    }
    let tty = shell_supervisor_request_bool(request, "tty");
    if tty {
        input = input
            .with_arg("pty_backend", "native-supervisor")
            .with_arg("supervisor_socket", socket.display().to_string())
            .with_arg("supervisor_epoch", epoch.to_string());
    }
    let output = TaskShellStartTool.execute(input)?;
    let task_id = shell_supervisor_summary_value(&output.summary, "task_id")
        .ok_or_else(|| app_error("shell supervisor start did not return a task_id"))?;
    let pty_backend = shell_supervisor_summary_value(&output.summary, "pty_backend")
        .unwrap_or_else(|| "none".to_string());
    Ok(ShellSupervisorStart {
        task_id,
        summary: output.summary,
        tty,
        pty_backend,
    })
}

fn shell_supervisor_wait_job(
    request: &ShellSupervisorRequest,
    supervisor_cwd: &Path,
) -> AppResult<crate::tools::types::ToolOutput> {
    let input = shell_supervisor_task_tool_input(request, supervisor_cwd, &["wait", "timeout_ms"])?;
    ExecShellWaitTool {
        tool_name: "exec_shell_wait",
    }
    .execute(input)
}

fn shell_supervisor_replay_job(
    request: &ShellSupervisorRequest,
    supervisor_cwd: &Path,
) -> AppResult<crate::tools::types::ToolOutput> {
    let input = shell_supervisor_task_tool_input(
        request,
        supervisor_cwd,
        &["stream", "offset", "cursor", "limit_bytes", "tail"],
    )?;
    ExecShellReplayTool.execute(input)
}

fn shell_supervisor_attach_job(
    request: &ShellSupervisorRequest,
    supervisor_cwd: &Path,
) -> AppResult<crate::tools::types::ToolOutput> {
    let input = shell_supervisor_task_tool_input(
        request,
        supervisor_cwd,
        &["offset", "cursor", "limit_bytes", "tail", "wait_ms"],
    )?;
    ExecShellAttachTool.execute(input)
}

fn shell_supervisor_stdin_job(
    request: &ShellSupervisorRequest,
    supervisor_cwd: &Path,
) -> AppResult<crate::tools::types::ToolOutput> {
    let input = shell_supervisor_task_tool_input(
        request,
        supervisor_cwd,
        &[
            "input",
            "stdin",
            "data",
            "input_base64",
            "bytes_base64",
            "close_stdin",
            "timeout_ms",
        ],
    )?;
    ExecShellInteractTool {
        tool_name: "exec_shell_interact",
    }
    .execute(input)
}

fn shell_supervisor_resize_job(
    request: &ShellSupervisorRequest,
    supervisor_cwd: &Path,
) -> AppResult<crate::tools::types::ToolOutput> {
    let input = shell_supervisor_task_tool_input(
        request,
        supervisor_cwd,
        &["tty_rows", "tty_cols", "rows", "cols"],
    )?;
    ExecShellResizeTool.execute(input)
}

fn shell_supervisor_cancel_job(
    request: &ShellSupervisorRequest,
    supervisor_cwd: &Path,
) -> AppResult<crate::tools::types::ToolOutput> {
    let cwd = shell_supervisor_request_cwd(request, supervisor_cwd)?;
    let mut input = ToolInput::new().with_arg("cwd", cwd.display().to_string());
    if shell_supervisor_request_bool(request, "all") {
        input = input.with_arg("all", "true");
    } else {
        let task_id = shell_supervisor_request_task_id(
            request,
            "shell supervisor cancel requires task_id or all=true",
        )?;
        input = input.with_arg("task_id", task_id);
    }
    ExecShellCancelTool.execute(input)
}

fn shell_supervisor_task_tool_input(
    request: &ShellSupervisorRequest,
    supervisor_cwd: &Path,
    optional_keys: &[&str],
) -> AppResult<ToolInput> {
    let task_id =
        shell_supervisor_request_task_id(request, "shell supervisor method requires task_id")?;
    let cwd = shell_supervisor_request_cwd(request, supervisor_cwd)?;
    let mut input = ToolInput::new()
        .with_arg("task_id", task_id)
        .with_arg("cwd", cwd.display().to_string());
    for key in optional_keys {
        if let Some(value) = shell_supervisor_request_scalar(request, key)? {
            input = input.with_arg((*key).to_string(), value);
        }
    }
    Ok(input)
}

fn shell_supervisor_request_task_id(
    request: &ShellSupervisorRequest,
    missing_message: &str,
) -> AppResult<String> {
    for key in ["task_id", "id"] {
        if let Some(value) = shell_supervisor_request_scalar(request, key)? {
            let value = value.trim().to_string();
            if !value.is_empty() {
                return Ok(value);
            }
        }
    }
    Err(app_error(missing_message))
}

fn shell_supervisor_request_cwd(
    request: &ShellSupervisorRequest,
    supervisor_cwd: &Path,
) -> AppResult<PathBuf> {
    let raw = shell_supervisor_request_string(request, "cwd")
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(".");
    let requested = Path::new(raw);
    let cwd = if requested.is_absolute() {
        requested.to_path_buf()
    } else {
        supervisor_cwd.join(requested)
    };
    let normalized = cwd
        .canonicalize()
        .unwrap_or_else(|_| normalize_child_path(supervisor_cwd, requested));
    let supervisor_root = supervisor_cwd
        .canonicalize()
        .unwrap_or_else(|_| supervisor_cwd.to_path_buf());
    if !normalized.starts_with(&supervisor_root) {
        return Err(app_error(format!(
            "shell supervisor cwd must stay inside {}",
            supervisor_root.display()
        )));
    }
    Ok(normalized)
}

fn normalize_child_path(root: &Path, child: &Path) -> PathBuf {
    let path = if child.is_absolute() {
        child.to_path_buf()
    } else {
        root.join(child)
    };
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                normalized.pop();
            }
            other => normalized.push(other.as_os_str()),
        }
    }
    normalized
}

fn shell_supervisor_request_string<'a>(
    request: &'a ShellSupervisorRequest,
    key: &str,
) -> Option<&'a str> {
    shell_supervisor_request_value(request, key).and_then(json_as_string)
}

fn shell_supervisor_request_scalar(
    request: &ShellSupervisorRequest,
    key: &str,
) -> AppResult<Option<String>> {
    let Some(value) = shell_supervisor_request_value(request, key) else {
        return Ok(None);
    };
    shell_supervisor_json_scalar(value)
        .map(Some)
        .ok_or_else(|| app_error(format!("shell supervisor request {key} must be scalar")))
}

fn shell_supervisor_request_bool(request: &ShellSupervisorRequest, key: &str) -> bool {
    match shell_supervisor_request_value(request, key) {
        Some(JsonValue::Bool(value)) => *value,
        Some(JsonValue::String(value)) => {
            matches!(value.trim(), "1" | "true" | "TRUE" | "yes" | "on")
        }
        Some(JsonValue::Number(value)) => value == "1",
        _ => false,
    }
}

fn shell_supervisor_request_u64(request: &ShellSupervisorRequest, key: &str) -> Option<u64> {
    shell_supervisor_request_value(request, key).and_then(json_as_u64)
}

fn shell_supervisor_request_object<'a>(
    request: &'a ShellSupervisorRequest,
    key: &str,
) -> Option<&'a BTreeMap<String, JsonValue>> {
    shell_supervisor_request_value(request, key).and_then(json_as_object)
}

fn shell_supervisor_request_value<'a>(
    request: &'a ShellSupervisorRequest,
    key: &str,
) -> Option<&'a JsonValue> {
    request.args.get(key).or_else(|| {
        ["params", "arguments"]
            .iter()
            .find_map(|container| request.args.get(*container).and_then(json_as_object))
            .and_then(|object| object.get(key))
    })
}

fn shell_supervisor_json_scalar(value: &JsonValue) -> Option<String> {
    match value {
        JsonValue::String(value) => Some(value.to_string()),
        JsonValue::Number(value) => Some(value.to_string()),
        JsonValue::Bool(value) => Some(value.to_string()),
        JsonValue::Null | JsonValue::Array(_) | JsonValue::Object(_) => None,
    }
}

fn shell_supervisor_summary_value(summary: &str, key: &str) -> Option<String> {
    summary.lines().find_map(|line| {
        line.strip_prefix(&format!("{key}: "))
            .or_else(|| line.strip_prefix(&format!("{key}=")))
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
    })
}

fn shell_supervisor_protocol_error_response(
    cwd: &Path,
    socket: &Path,
    epoch: &str,
    error: &str,
) -> JsonValue {
    JsonValue::Object(BTreeMap::from([
        (
            "kind".to_string(),
            JsonValue::String("deepseek.exec_shell.supervisor.response.v1".to_string()),
        ),
        (
            "method".to_string(),
            JsonValue::String("invalid_request".to_string()),
        ),
        ("status".to_string(), JsonValue::String("error".to_string())),
        (
            "cwd".to_string(),
            JsonValue::String(cwd.display().to_string()),
        ),
        (
            "supervisor_pid".to_string(),
            JsonValue::Number(std::process::id().to_string()),
        ),
        (
            "supervisor_socket".to_string(),
            JsonValue::String(socket.display().to_string()),
        ),
        (
            "supervisor_epoch".to_string(),
            JsonValue::String(epoch.to_string()),
        ),
        (
            "protocol".to_string(),
            JsonValue::String("newline-json-v1".to_string()),
        ),
        (
            "methods".to_string(),
            shell_supervisor_method_json(SHELL_SUPERVISOR_SUPPORTED_METHODS),
        ),
        (
            "unsupported_methods".to_string(),
            shell_supervisor_method_json(SHELL_SUPERVISOR_UNSUPPORTED_PTY_METHODS),
        ),
        (
            "pty_backend".to_string(),
            JsonValue::String("none".to_string()),
        ),
        ("native_pty".to_string(), JsonValue::Bool(false)),
        (
            "active_jobs".to_string(),
            JsonValue::Number("0".to_string()),
        ),
        ("error".to_string(), JsonValue::String(error.to_string())),
    ]))
}

#[cfg(windows)]
fn shell_supervisor_stream_unsupported_response(
    cwd: &Path,
    socket: &Path,
    epoch: &str,
    method: &str,
    stream_method: &str,
    error: &str,
) -> JsonValue {
    let mut response = shell_supervisor_protocol_error_response(cwd, socket, epoch, error);
    if let JsonValue::Object(root) = &mut response {
        root.insert("method".to_string(), JsonValue::String(method.to_string()));
        root.insert(
            "stream_method".to_string(),
            JsonValue::String(stream_method.to_string()),
        );
        root.insert("stream_done".to_string(), JsonValue::Bool(true));
    }
    response
}

#[cfg(unix)]
fn write_shell_supervisor_manifest(cwd: &Path, socket: &Path, epoch: &str) -> AppResult<()> {
    let active_jobs = count_active_durable_shell_jobs(cwd)?;
    write_shell_supervisor_manifest_snapshot(cwd, socket, epoch, epoch, active_jobs)
}

fn refresh_shell_supervisor_manifest_if_present(
    cwd: &Path,
    socket: &Path,
    epoch: &str,
    active_jobs: u64,
) -> AppResult<()> {
    if !cwd.join(".dscode/shell-supervisor").is_dir() {
        return Ok(());
    }
    let updated_at = format_epoch_seconds(current_epoch_seconds());
    write_shell_supervisor_manifest_snapshot(cwd, socket, epoch, &updated_at, active_jobs)
}

fn write_shell_supervisor_manifest_snapshot(
    cwd: &Path,
    socket: &Path,
    epoch: &str,
    updated_at: &str,
    active_jobs: u64,
) -> AppResult<()> {
    let manifest = JsonValue::Object(BTreeMap::from([
        (
            "kind".to_string(),
            JsonValue::String("deepseek.exec_shell.supervisor.v1".to_string()),
        ),
        (
            "supervisor_pid".to_string(),
            JsonValue::Number(std::process::id().to_string()),
        ),
        (
            "supervisor_socket".to_string(),
            JsonValue::String(socket.display().to_string()),
        ),
        (
            "supervisor_epoch".to_string(),
            JsonValue::String(epoch.to_string()),
        ),
        (
            "protocol".to_string(),
            JsonValue::String("newline-json-v1".to_string()),
        ),
        (
            "methods".to_string(),
            shell_supervisor_method_json(SHELL_SUPERVISOR_SUPPORTED_METHODS),
        ),
        (
            "unsupported_methods".to_string(),
            shell_supervisor_method_json(SHELL_SUPERVISOR_UNSUPPORTED_PTY_METHODS),
        ),
        (
            "active_jobs".to_string(),
            JsonValue::Number(active_jobs.to_string()),
        ),
        (
            "started_at".to_string(),
            JsonValue::String(epoch.to_string()),
        ),
        (
            "updated_at".to_string(),
            JsonValue::String(updated_at.to_string()),
        ),
        ("control_token_hash".to_string(), JsonValue::Null),
    ]));
    std::fs::write(
        cwd.join(".dscode/shell-supervisor/manifest.json"),
        json_value_to_string(&manifest),
    )?;
    Ok(())
}

fn shell_supervisor_method_json(methods: &[&str]) -> JsonValue {
    JsonValue::Array(
        methods
            .iter()
            .map(|method| JsonValue::String((*method).to_string()))
            .collect(),
    )
}

fn shell_supervisor_event_json(
    event: &str,
    cwd: &Path,
    socket: &Path,
    epoch: &str,
    message: Option<&str>,
) -> JsonValue {
    let mut object = BTreeMap::from([
        (
            "kind".to_string(),
            JsonValue::String("deepseek.exec_shell.supervisor_daemon.v1".to_string()),
        ),
        ("event".to_string(), JsonValue::String(event.to_string())),
        (
            "cwd".to_string(),
            JsonValue::String(cwd.display().to_string()),
        ),
        (
            "socket".to_string(),
            JsonValue::String(socket.display().to_string()),
        ),
        ("epoch".to_string(), JsonValue::String(epoch.to_string())),
    ]);
    if let Some(message) = message {
        object.insert(
            "message".to_string(),
            JsonValue::String(message.to_string()),
        );
    }
    JsonValue::Object(object)
}

fn render_agent_services(args: AgentsServiceArgs) -> AppResult<()> {
    let config = service_template_config(args)?;
    let templates = service_templates(&config);

    if let Some(out) = &config.out {
        for template in &templates {
            let path = out.join(template.path);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&path, &template.body)?;
            println!("wrote {}", path.display());
        }
        let guide_path = out.join("SERVICES.md");
        std::fs::write(&guide_path, service_lifecycle_guide(&config, &templates))?;
        println!("wrote {}", guide_path.display());
        print_service_next_steps(config.kind, out);
    } else {
        for template in &templates {
            println!("--- {} ---", template.path);
            print!("{}", template.body);
            if !template.body.ends_with('\n') {
                println!();
            }
        }
    }
    Ok(())
}

fn service_template_config(args: AgentsServiceArgs) -> AppResult<ServiceTemplateConfig> {
    let bin = match args.bin {
        Some(bin) => normalize_service_path_or_command(bin),
        None => std::env::current_exe()?.display().to_string(),
    };
    let workdir = match args.workdir {
        Some(workdir) => normalize_service_path_or_command(workdir),
        None => std::env::current_dir()?.display().to_string(),
    };
    Ok(ServiceTemplateConfig {
        kind: args.kind,
        out: args.out.map(PathBuf::from),
        bin,
        workdir,
        addr: args.addr,
        interval_ms: args.interval_ms.max(100),
        budget: args.budget,
    })
}

fn normalize_service_path_or_command(value: String) -> String {
    let path = PathBuf::from(&value);
    if path.components().count() > 1 {
        path.display().to_string()
    } else {
        value
    }
}

fn service_templates(config: &ServiceTemplateConfig) -> Vec<ServiceTemplate> {
    let mut templates = Vec::new();
    if matches!(
        config.kind,
        AgentsServiceKind::Systemd | AgentsServiceKind::All
    ) {
        templates.push(ServiceTemplate {
            path: "systemd/deepseek-runtime.service",
            body: systemd_runtime_service(config),
        });
        templates.push(ServiceTemplate {
            path: "systemd/deepseek-agents.service",
            body: systemd_agents_service(config),
        });
        templates.push(ServiceTemplate {
            path: "systemd/deepseek-diagnostics.service",
            body: systemd_diagnostics_service(config),
        });
        templates.push(ServiceTemplate {
            path: "systemd/deepseek-shell-supervisor.service",
            body: systemd_shell_supervisor_service(config),
        });
    }
    if matches!(
        config.kind,
        AgentsServiceKind::Launchd | AgentsServiceKind::All
    ) {
        templates.push(ServiceTemplate {
            path: "launchd/com.deepseek.runtime.plist",
            body: launchd_runtime_service(config),
        });
        templates.push(ServiceTemplate {
            path: "launchd/com.deepseek.agents.plist",
            body: launchd_agents_service(config),
        });
        templates.push(ServiceTemplate {
            path: "launchd/com.deepseek.diagnostics.plist",
            body: launchd_diagnostics_service(config),
        });
        templates.push(ServiceTemplate {
            path: "launchd/com.deepseek.shell-supervisor.plist",
            body: launchd_shell_supervisor_service(config),
        });
    }
    templates
}

fn service_lifecycle_guide(
    config: &ServiceTemplateConfig,
    templates: &[ServiceTemplate],
) -> String {
    let mut out = String::new();
    out.push_str("# DeepSeekCode Service Lifecycle\n\n");
    out.push_str("Generated by `deepseek agents service` for this workspace.\n\n");
    out.push_str("## Target\n\n");
    out.push_str(&format!("- binary: `{}`\n", config.bin));
    out.push_str(&format!("- workdir: `{}`\n", config.workdir));
    out.push_str(&format!("- runtime address: `{}`\n", config.addr));
    out.push_str(&format!("- worker interval: `{}` ms\n", config.interval_ms));
    if let Some(budget) = config.budget {
        out.push_str(&format!("- daemon budget: `{budget}`\n"));
    }
    out.push_str("\n## Files\n\n");
    for template in templates {
        out.push_str(&format!("- `{}`\n", template.path));
    }
    if matches!(
        config.kind,
        AgentsServiceKind::Systemd | AgentsServiceKind::All
    ) {
        out.push_str("\n## systemd User Lifecycle\n\n");
        out.push_str("```bash\n");
        out.push_str("mkdir -p ~/.config/systemd/user\n");
        out.push_str("cp systemd/*.service ~/.config/systemd/user/\n");
        out.push_str("systemctl --user daemon-reload\n");
        out.push_str("systemctl --user enable --now deepseek-runtime.service deepseek-agents.service deepseek-diagnostics.service deepseek-shell-supervisor.service\n");
        out.push_str("systemctl --user status deepseek-runtime.service deepseek-agents.service deepseek-diagnostics.service deepseek-shell-supervisor.service\n");
        out.push_str("journalctl --user -u deepseek-runtime.service -u deepseek-agents.service -u deepseek-diagnostics.service -u deepseek-shell-supervisor.service -f\n");
        out.push_str("systemctl --user restart deepseek-runtime.service deepseek-agents.service deepseek-diagnostics.service deepseek-shell-supervisor.service\n");
        out.push_str("systemctl --user stop deepseek-runtime.service deepseek-agents.service deepseek-diagnostics.service deepseek-shell-supervisor.service\n");
        out.push_str("systemctl --user disable deepseek-runtime.service deepseek-agents.service deepseek-diagnostics.service deepseek-shell-supervisor.service\n");
        out.push_str("```\n");
    }
    if matches!(
        config.kind,
        AgentsServiceKind::Launchd | AgentsServiceKind::All
    ) {
        out.push_str("\n## launchd User Lifecycle\n\n");
        out.push_str("```bash\n");
        out.push_str("mkdir -p ~/Library/LaunchAgents\n");
        out.push_str("cp launchd/*.plist ~/Library/LaunchAgents/\n");
        out.push_str("launchctl load -w ~/Library/LaunchAgents/com.deepseek.runtime.plist ~/Library/LaunchAgents/com.deepseek.agents.plist ~/Library/LaunchAgents/com.deepseek.diagnostics.plist ~/Library/LaunchAgents/com.deepseek.shell-supervisor.plist\n");
        out.push_str("launchctl list | grep com.deepseek\n");
        out.push_str("tail -f /tmp/deepseek-runtime.out.log /tmp/deepseek-agents.out.log /tmp/deepseek-diagnostics.out.log /tmp/deepseek-shell-supervisor.out.log\n");
        out.push_str("launchctl kickstart -k gui/$(id -u)/com.deepseek.runtime gui/$(id -u)/com.deepseek.agents gui/$(id -u)/com.deepseek.diagnostics gui/$(id -u)/com.deepseek.shell-supervisor\n");
        out.push_str("launchctl unload -w ~/Library/LaunchAgents/com.deepseek.runtime.plist ~/Library/LaunchAgents/com.deepseek.agents.plist ~/Library/LaunchAgents/com.deepseek.diagnostics.plist ~/Library/LaunchAgents/com.deepseek.shell-supervisor.plist\n");
        out.push_str("```\n");
    }
    out.push_str("\n## Runtime Checks\n\n");
    out.push_str("```bash\n");
    out.push_str(&format!("curl -fsS http://{}/v1/health\n", config.addr));
    out.push_str(&format!("{} doctor --json\n", config.bin));
    out.push_str(&format!("{} agents rlm-status --json\n", config.bin));
    out.push_str(&format!("{} agents shell status --json\n", config.bin));
    out.push_str(&format!("{} diagnostics --changed --json\n", config.bin));
    out.push_str("```\n");
    out
}

fn systemd_runtime_service(config: &ServiceTemplateConfig) -> String {
    format!(
        "[Unit]\n\
Description=DeepSeekCode HTTP runtime\n\
After=network.target\n\
\n\
[Service]\n\
Type=simple\n\
WorkingDirectory={workdir}\n\
ExecStart=/usr/bin/env {bin} serve --http --addr {addr}\n\
Restart=on-failure\n\
RestartSec=5\n\
\n\
[Install]\n\
WantedBy=default.target\n",
        workdir = systemd_quote(&config.workdir),
        bin = systemd_quote(&config.bin),
        addr = systemd_quote(&config.addr)
    )
}

fn systemd_agents_service(config: &ServiceTemplateConfig) -> String {
    let budget = config
        .budget
        .map(|budget| format!(" --budget {budget}"))
        .unwrap_or_default();
    format!(
        "[Unit]\n\
Description=DeepSeekCode runtime task daemon\n\
# Runs due automations, pending runtime tasks, stale RLM recovery, and one queued live RLM turn per tick.\n\
After=network.target deepseek-runtime.service\n\
\n\
[Service]\n\
Type=simple\n\
WorkingDirectory={workdir}\n\
ExecStart=/usr/bin/env {bin} agents daemon --interval-ms {interval_ms}{budget} --json\n\
Restart=on-failure\n\
RestartSec=5\n\
\n\
[Install]\n\
WantedBy=default.target\n",
        workdir = systemd_quote(&config.workdir),
        bin = systemd_quote(&config.bin),
        interval_ms = config.interval_ms,
        budget = budget,
    )
}

fn systemd_diagnostics_service(config: &ServiceTemplateConfig) -> String {
    format!(
        "[Unit]\n\
Description=DeepSeekCode diagnostics watch worker\n\
After=network.target\n\
\n\
[Service]\n\
Type=simple\n\
WorkingDirectory={workdir}\n\
ExecStart=/usr/bin/env {bin} diagnostics --watch --changed --interval-ms {interval_ms} --json\n\
Restart=on-failure\n\
RestartSec=5\n\
\n\
[Install]\n\
WantedBy=default.target\n",
        workdir = systemd_quote(&config.workdir),
        bin = systemd_quote(&config.bin),
        interval_ms = config.interval_ms,
    )
}

fn systemd_shell_supervisor_service(config: &ServiceTemplateConfig) -> String {
    format!(
        "[Unit]\n\
Description=DeepSeekCode shell supervisor protocol bridge\n\
# Exposes workspace-local shell status/show/start/control, including native-supervisor PTY jobs where supported.\n\
After=network.target\n\
\n\
[Service]\n\
Type=simple\n\
WorkingDirectory={workdir}\n\
ExecStart=/usr/bin/env {bin} agents shell-supervisor --json\n\
Restart=on-failure\n\
RestartSec=5\n\
\n\
[Install]\n\
WantedBy=default.target\n",
        workdir = systemd_quote(&config.workdir),
        bin = systemd_quote(&config.bin),
    )
}

fn launchd_runtime_service(config: &ServiceTemplateConfig) -> String {
    launchd_plist(
        "com.deepseek.runtime",
        &config.workdir,
        &[
            "/usr/bin/env".to_string(),
            config.bin.clone(),
            "serve".to_string(),
            "--http".to_string(),
            "--addr".to_string(),
            config.addr.clone(),
        ],
        "/tmp/deepseek-runtime.out.log",
        "/tmp/deepseek-runtime.err.log",
        None,
    )
}

fn launchd_agents_service(config: &ServiceTemplateConfig) -> String {
    let mut args = vec![
        "/usr/bin/env".to_string(),
        config.bin.clone(),
        "agents".to_string(),
        "daemon".to_string(),
        "--interval-ms".to_string(),
        config.interval_ms.to_string(),
    ];
    if let Some(budget) = config.budget {
        args.push("--budget".to_string());
        args.push(budget.to_string());
    }
    args.push("--json".to_string());
    launchd_plist(
        "com.deepseek.agents",
        &config.workdir,
        &args,
        "/tmp/deepseek-agents.out.log",
        "/tmp/deepseek-agents.err.log",
        Some(
            "Runs due automations, pending runtime tasks, stale RLM recovery, and one queued live RLM turn per tick.",
        ),
    )
}

fn launchd_diagnostics_service(config: &ServiceTemplateConfig) -> String {
    launchd_plist(
        "com.deepseek.diagnostics",
        &config.workdir,
        &[
            "/usr/bin/env".to_string(),
            config.bin.clone(),
            "diagnostics".to_string(),
            "--watch".to_string(),
            "--changed".to_string(),
            "--interval-ms".to_string(),
            config.interval_ms.to_string(),
            "--json".to_string(),
        ],
        "/tmp/deepseek-diagnostics.out.log",
        "/tmp/deepseek-diagnostics.err.log",
        None,
    )
}

fn launchd_shell_supervisor_service(config: &ServiceTemplateConfig) -> String {
    launchd_plist(
        "com.deepseek.shell-supervisor",
        &config.workdir,
        &[
            "/usr/bin/env".to_string(),
            config.bin.clone(),
            "agents".to_string(),
            "shell-supervisor".to_string(),
            "--json".to_string(),
        ],
        "/tmp/deepseek-shell-supervisor.out.log",
        "/tmp/deepseek-shell-supervisor.err.log",
        Some(
            "Exposes workspace-local shell status/show/start/control, including native-supervisor PTY jobs where supported.",
        ),
    )
}

fn launchd_plist(
    label: &str,
    workdir: &str,
    args: &[String],
    stdout_path: &str,
    stderr_path: &str,
    comment: Option<&str>,
) -> String {
    let mut body = String::new();
    body.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    body.push_str("<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" ");
    body.push_str("\"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n");
    body.push_str("<plist version=\"1.0\">\n<dict>\n");
    if let Some(comment) = comment {
        body.push_str(&format!("  <!-- {} -->\n", xml_escape(comment)));
    }
    body.push_str("  <key>Label</key>\n");
    body.push_str(&format!("  <string>{}</string>\n", xml_escape(label)));
    body.push_str("  <key>WorkingDirectory</key>\n");
    body.push_str(&format!("  <string>{}</string>\n", xml_escape(workdir)));
    body.push_str("  <key>ProgramArguments</key>\n  <array>\n");
    for arg in args {
        body.push_str(&format!("    <string>{}</string>\n", xml_escape(arg)));
    }
    body.push_str("  </array>\n");
    body.push_str("  <key>RunAtLoad</key>\n  <true/>\n");
    body.push_str("  <key>KeepAlive</key>\n  <true/>\n");
    body.push_str("  <key>StandardOutPath</key>\n");
    body.push_str(&format!("  <string>{}</string>\n", xml_escape(stdout_path)));
    body.push_str("  <key>StandardErrorPath</key>\n");
    body.push_str(&format!("  <string>{}</string>\n", xml_escape(stderr_path)));
    body.push_str("</dict>\n</plist>\n");
    body
}

fn systemd_quote(value: &str) -> String {
    if !value.is_empty()
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '/' | '.' | '_' | '-' | ':' | '%'))
    {
        value.to_string()
    } else {
        format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
    }
}

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn print_service_next_steps(kind: AgentsServiceKind, out: &Path) {
    match kind {
        AgentsServiceKind::Systemd => {
            println!(
                "next: review SERVICES.md, install systemd/*.service into ~/.config/systemd/user, then run `systemctl --user daemon-reload`"
            );
        }
        AgentsServiceKind::Launchd => {
            println!(
                "next: review SERVICES.md, install launchd/*.plist into ~/Library/LaunchAgents, then load with `launchctl load -w <plist>`"
            );
        }
        AgentsServiceKind::All => {
            println!(
                "next: review {}/SERVICES.md, choose the files for your supervisor, and install them with the platform tool",
                out.display()
            );
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ServiceDoctorCheck {
    status: ServiceDoctorStatus,
    name: String,
    message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ServiceDoctorStatus {
    Ok,
    Warn,
    Blocker,
}

#[derive(Debug, Clone)]
struct ServiceDoctorReport {
    config: ServiceTemplateConfig,
    installed: bool,
    checks: Vec<ServiceDoctorCheck>,
}

fn run_service_doctor(args: AgentsServiceDoctorArgs) -> AppResult<()> {
    let json = args.json;
    let config = service_template_config(AgentsServiceArgs {
        kind: args.kind,
        out: args.out,
        bin: args.bin,
        workdir: args.workdir,
        addr: args.addr,
        interval_ms: args.interval_ms,
        budget: args.budget,
    })?;
    let report = if args.installed {
        build_service_doctor_report_with_options(config, true)
    } else {
        build_service_doctor_report(config)
    };
    if json {
        println!("{}", render_service_doctor_json(&report));
    } else {
        print!("{}", render_service_doctor_text(&report));
    }
    if report.blocker_count() > 0 {
        return Err(app_error(format!(
            "service doctor found {} blocker(s)",
            report.blocker_count()
        )));
    }
    Ok(())
}

impl ServiceDoctorReport {
    fn blocker_count(&self) -> usize {
        self.checks
            .iter()
            .filter(|check| check.status == ServiceDoctorStatus::Blocker)
            .count()
    }

    fn warning_count(&self) -> usize {
        self.checks
            .iter()
            .filter(|check| check.status == ServiceDoctorStatus::Warn)
            .count()
    }
}

fn build_service_doctor_report(config: ServiceTemplateConfig) -> ServiceDoctorReport {
    build_service_doctor_report_with_options(config, false)
}

fn build_service_doctor_report_with_options(
    config: ServiceTemplateConfig,
    installed: bool,
) -> ServiceDoctorReport {
    let templates = service_templates(&config);
    let mut checks = Vec::new();

    service_doctor_check_binary(&config, &mut checks);
    service_doctor_check_workdir(&config, &mut checks);
    service_doctor_check_template_topology(&config, &templates, &mut checks);
    service_doctor_check_platform_tools(config.kind, &mut checks);
    if installed {
        service_doctor_check_installed_services(config.kind, &mut checks);
    }
    service_doctor_check_output_dir(&config, &templates, &mut checks);

    ServiceDoctorReport {
        config,
        installed,
        checks,
    }
}

fn service_doctor_check_binary(
    config: &ServiceTemplateConfig,
    checks: &mut Vec<ServiceDoctorCheck>,
) {
    let binary = config.bin.trim();
    if binary.is_empty() {
        push_service_doctor_check(
            checks,
            ServiceDoctorStatus::Blocker,
            "binary",
            "service binary is empty",
        );
        return;
    }
    if service_value_is_path(binary) {
        let path = Path::new(binary);
        if path.is_file() {
            push_service_doctor_check(
                checks,
                ServiceDoctorStatus::Ok,
                "binary",
                format!("binary path exists: {binary}"),
            );
        } else {
            push_service_doctor_check(
                checks,
                ServiceDoctorStatus::Blocker,
                "binary",
                format!("binary path is missing or not a file: {binary}"),
            );
        }
    } else if command_on_path(binary) {
        push_service_doctor_check(
            checks,
            ServiceDoctorStatus::Ok,
            "binary",
            format!("binary command is on PATH: {binary}"),
        );
    } else {
        push_service_doctor_check(
            checks,
            ServiceDoctorStatus::Blocker,
            "binary",
            format!("binary command is not on PATH: {binary}"),
        );
    }
}

fn service_doctor_check_workdir(
    config: &ServiceTemplateConfig,
    checks: &mut Vec<ServiceDoctorCheck>,
) {
    let workdir = config.workdir.trim();
    if workdir.is_empty() {
        push_service_doctor_check(
            checks,
            ServiceDoctorStatus::Blocker,
            "workdir",
            "service workdir is empty",
        );
        return;
    }
    let path = Path::new(workdir);
    if path.is_dir() {
        push_service_doctor_check(
            checks,
            ServiceDoctorStatus::Ok,
            "workdir",
            format!("workspace directory exists: {workdir}"),
        );
    } else {
        push_service_doctor_check(
            checks,
            ServiceDoctorStatus::Blocker,
            "workdir",
            format!("workspace directory is missing or not a directory: {workdir}"),
        );
    }
}

fn service_doctor_check_template_topology(
    config: &ServiceTemplateConfig,
    templates: &[ServiceTemplate],
    checks: &mut Vec<ServiceDoctorCheck>,
) {
    let expected = expected_service_template_paths(config.kind);
    for path in &expected {
        if templates.iter().any(|template| template.path == *path) {
            push_service_doctor_check(
                checks,
                ServiceDoctorStatus::Ok,
                "template_set",
                format!("template is rendered: {path}"),
            );
        } else {
            push_service_doctor_check(
                checks,
                ServiceDoctorStatus::Blocker,
                "template_set",
                format!("template is missing from render set: {path}"),
            );
        }
    }

    service_doctor_check_template_command(
        templates,
        "runtime_service",
        "runtime",
        &["serve", "--http"],
        checks,
    );
    service_doctor_check_template_command(
        templates,
        "agents_service",
        "agents",
        &["agents", "daemon"],
        checks,
    );
    service_doctor_check_template_command(
        templates,
        "diagnostics_service",
        "diagnostics",
        &["diagnostics", "--watch"],
        checks,
    );
    service_doctor_check_template_command(
        templates,
        "shell_supervisor_service",
        "shell-supervisor",
        &["agents", "shell-supervisor", "--json"],
        checks,
    );
    service_doctor_check_template_command_vectors(config, templates, checks);
}

fn service_doctor_check_template_command(
    templates: &[ServiceTemplate],
    name: &str,
    path_marker: &str,
    tokens: &[&str],
    checks: &mut Vec<ServiceDoctorCheck>,
) {
    let matching = templates
        .iter()
        .filter(|template| template.path.contains(path_marker))
        .collect::<Vec<_>>();
    if matching.is_empty() {
        push_service_doctor_check(
            checks,
            ServiceDoctorStatus::Blocker,
            name,
            format!("no template path contains `{path_marker}`"),
        );
        return;
    }
    if matching
        .iter()
        .all(|template| tokens.iter().all(|token| template.body.contains(token)))
    {
        push_service_doctor_check(
            checks,
            ServiceDoctorStatus::Ok,
            name,
            format!("all {path_marker} templates contain {}", tokens.join(" ")),
        );
    } else {
        push_service_doctor_check(
            checks,
            ServiceDoctorStatus::Blocker,
            name,
            format!(
                "one or more {path_marker} templates are missing {}",
                tokens.join(" ")
            ),
        );
    }
}

fn service_doctor_check_template_command_vectors(
    config: &ServiceTemplateConfig,
    templates: &[ServiceTemplate],
    checks: &mut Vec<ServiceDoctorCheck>,
) {
    let mut blockers = Vec::new();
    for template in templates {
        match service_template_command_vector(template) {
            Ok(command) => {
                if command.workdir != config.workdir {
                    blockers.push(format!(
                        "{} has workdir `{}`, expected `{}`",
                        template.path, command.workdir, config.workdir
                    ));
                    continue;
                }
                let Some(expected) = expected_service_command_args(config, template.path) else {
                    continue;
                };
                if command.argv != expected {
                    blockers.push(format!(
                        "{} command `{}` did not match expected `{}`",
                        template.path,
                        command.argv.join(" "),
                        expected.join(" ")
                    ));
                }
            }
            Err(error) => blockers.push(format!("{}: {error}", template.path)),
        }
    }
    if blockers.is_empty() {
        push_service_doctor_check(
            checks,
            ServiceDoctorStatus::Ok,
            "template_command_vectors",
            "all generated service templates parse to the expected argv/workdir",
        );
    } else {
        push_service_doctor_check(
            checks,
            ServiceDoctorStatus::Blocker,
            "template_command_vectors",
            blockers.join("; "),
        );
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ServiceTemplateCommand {
    workdir: String,
    argv: Vec<String>,
}

fn service_template_command_vector(
    template: &ServiceTemplate,
) -> AppResult<ServiceTemplateCommand> {
    if template.path.ends_with(".service") {
        service_template_command_vector_systemd(template)
    } else if template.path.ends_with(".plist") {
        service_template_command_vector_launchd(template)
    } else {
        Err(app_error(format!(
            "unsupported service template kind: {}",
            template.path
        )))
    }
}

fn service_template_command_vector_systemd(
    template: &ServiceTemplate,
) -> AppResult<ServiceTemplateCommand> {
    let workdir = service_template_systemd_value(&template.body, "WorkingDirectory")
        .ok_or_else(|| app_error("systemd template missing WorkingDirectory"))?;
    let command = service_template_systemd_value(&template.body, "ExecStart")
        .ok_or_else(|| app_error("systemd template missing ExecStart"))?;
    let mut argv = split_service_command_line(&command)?;
    if argv.first().is_some_and(|arg| arg == "/usr/bin/env") {
        argv.remove(0);
    }
    Ok(ServiceTemplateCommand { workdir, argv })
}

fn service_template_systemd_value(body: &str, key: &str) -> Option<String> {
    let prefix = format!("{key}=");
    body.lines()
        .find_map(|line| line.strip_prefix(&prefix).map(systemd_unquote_value))
}

fn systemd_unquote_value(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.len() >= 2 && trimmed.starts_with('"') && trimmed.ends_with('"') {
        let inner = &trimmed[1..trimmed.len() - 1];
        let mut out = String::new();
        let mut escaped = false;
        for ch in inner.chars() {
            if escaped {
                out.push(ch);
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else {
                out.push(ch);
            }
        }
        if escaped {
            out.push('\\');
        }
        out
    } else {
        trimmed.to_string()
    }
}

fn split_service_command_line(value: &str) -> AppResult<Vec<String>> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    let mut escaped = false;
    let mut saw_token = false;
    for ch in value.chars() {
        if escaped {
            current.push(ch);
            saw_token = true;
            escaped = false;
            continue;
        }
        match ch {
            '\\' => {
                escaped = true;
                saw_token = true;
            }
            '"' => {
                in_quotes = !in_quotes;
                saw_token = true;
            }
            ch if ch.is_whitespace() && !in_quotes => {
                if saw_token {
                    args.push(std::mem::take(&mut current));
                    saw_token = false;
                }
            }
            _ => {
                current.push(ch);
                saw_token = true;
            }
        }
    }
    if escaped {
        current.push('\\');
    }
    if in_quotes {
        return Err(app_error("unterminated quoted service command"));
    }
    if saw_token {
        args.push(current);
    }
    if args.is_empty() {
        return Err(app_error("service command is empty"));
    }
    Ok(args)
}

fn service_template_command_vector_launchd(
    template: &ServiceTemplate,
) -> AppResult<ServiceTemplateCommand> {
    let workdir = launchd_string_after_key(&template.body, "WorkingDirectory")
        .ok_or_else(|| app_error("launchd template missing WorkingDirectory"))?;
    let args_block = launchd_array_after_key(&template.body, "ProgramArguments")
        .ok_or_else(|| app_error("launchd template missing ProgramArguments"))?;
    let mut argv = launchd_strings(args_block);
    if argv.first().is_some_and(|arg| arg == "/usr/bin/env") {
        argv.remove(0);
    }
    if argv.is_empty() {
        return Err(app_error("launchd ProgramArguments is empty"));
    }
    Ok(ServiceTemplateCommand { workdir, argv })
}

fn launchd_string_after_key(body: &str, key: &str) -> Option<String> {
    let marker = format!("<key>{}</key>", xml_escape(key));
    let tail = body.split_once(&marker)?.1;
    let start = tail.find("<string>")? + "<string>".len();
    let end = tail[start..].find("</string>")? + start;
    Some(xml_unescape(&tail[start..end]))
}

fn launchd_array_after_key<'a>(body: &'a str, key: &str) -> Option<&'a str> {
    let marker = format!("<key>{}</key>", xml_escape(key));
    let tail = body.split_once(&marker)?.1;
    let start = tail.find("<array>")? + "<array>".len();
    let end = tail[start..].find("</array>")? + start;
    Some(&tail[start..end])
}

fn launchd_strings(body: &str) -> Vec<String> {
    let mut values = Vec::new();
    let mut rest = body;
    while let Some((_, after_start)) = rest.split_once("<string>") {
        let Some((value, after_end)) = after_start.split_once("</string>") else {
            break;
        };
        values.push(xml_unescape(value));
        rest = after_end;
    }
    values
}

fn xml_unescape(value: &str) -> String {
    value
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}

fn expected_service_command_args(
    config: &ServiceTemplateConfig,
    path: &str,
) -> Option<Vec<String>> {
    if path.contains("runtime") {
        Some(vec![
            config.bin.clone(),
            "serve".to_string(),
            "--http".to_string(),
            "--addr".to_string(),
            config.addr.clone(),
        ])
    } else if path.contains("agents") {
        let mut args = vec![
            config.bin.clone(),
            "agents".to_string(),
            "daemon".to_string(),
            "--interval-ms".to_string(),
            config.interval_ms.to_string(),
        ];
        if let Some(budget) = config.budget {
            args.push("--budget".to_string());
            args.push(budget.to_string());
        }
        args.push("--json".to_string());
        Some(args)
    } else if path.contains("diagnostics") {
        Some(vec![
            config.bin.clone(),
            "diagnostics".to_string(),
            "--watch".to_string(),
            "--changed".to_string(),
            "--interval-ms".to_string(),
            config.interval_ms.to_string(),
            "--json".to_string(),
        ])
    } else if path.contains("shell-supervisor") {
        Some(vec![
            config.bin.clone(),
            "agents".to_string(),
            "shell-supervisor".to_string(),
            "--json".to_string(),
        ])
    } else {
        None
    }
}

fn service_doctor_check_platform_tools(
    kind: AgentsServiceKind,
    checks: &mut Vec<ServiceDoctorCheck>,
) {
    if matches!(kind, AgentsServiceKind::Systemd | AgentsServiceKind::All) {
        if command_on_path("systemctl") {
            push_service_doctor_check(
                checks,
                ServiceDoctorStatus::Ok,
                "systemd",
                "systemctl is on PATH",
            );
        } else {
            push_service_doctor_check(
                checks,
                ServiceDoctorStatus::Warn,
                "systemd",
                "systemctl is not on PATH; generated systemd files can still be reviewed",
            );
        }
    }
    if matches!(kind, AgentsServiceKind::Launchd | AgentsServiceKind::All) {
        if command_on_path("launchctl") {
            push_service_doctor_check(
                checks,
                ServiceDoctorStatus::Ok,
                "launchd",
                "launchctl is on PATH",
            );
        } else {
            push_service_doctor_check(
                checks,
                ServiceDoctorStatus::Warn,
                "launchd",
                "launchctl is not on PATH; generated launchd files can still be reviewed",
            );
        }
    }
}

fn service_doctor_check_installed_services(
    kind: AgentsServiceKind,
    checks: &mut Vec<ServiceDoctorCheck>,
) {
    if matches!(kind, AgentsServiceKind::Systemd | AgentsServiceKind::All) {
        service_doctor_check_installed_systemd_services(checks);
    }
    if matches!(kind, AgentsServiceKind::Launchd | AgentsServiceKind::All) {
        service_doctor_check_installed_launchd_services(checks);
    }
}

const SYSTEMD_SERVICE_UNITS: &[&str] = &[
    "deepseek-runtime.service",
    "deepseek-agents.service",
    "deepseek-diagnostics.service",
    "deepseek-shell-supervisor.service",
];

const LAUNCHD_SERVICE_LABELS: &[&str] = &[
    "com.deepseek.runtime",
    "com.deepseek.agents",
    "com.deepseek.diagnostics",
    "com.deepseek.shell-supervisor",
];

#[derive(Debug, Clone, PartialEq, Eq)]
struct SystemdInstalledServiceStatus {
    unit: String,
    load_state: String,
    active_state: String,
    sub_state: String,
    unit_file_state: String,
    fragment_path: String,
}

impl SystemdInstalledServiceStatus {
    fn is_ready(&self) -> bool {
        self.load_state == "loaded"
            && self.active_state == "active"
            && matches!(
                self.unit_file_state.as_str(),
                "enabled" | "enabled-runtime" | "linked" | "linked-runtime" | "static"
            )
    }

    fn summary(&self) -> String {
        format!(
            "load_state={} active_state={} sub_state={} unit_file_state={} fragment_path={}",
            service_status_value(&self.load_state),
            service_status_value(&self.active_state),
            service_status_value(&self.sub_state),
            service_status_value(&self.unit_file_state),
            service_status_value(&self.fragment_path)
        )
    }
}

fn service_doctor_check_installed_systemd_services(checks: &mut Vec<ServiceDoctorCheck>) {
    if !command_on_path("systemctl") {
        push_service_doctor_check(
            checks,
            ServiceDoctorStatus::Blocker,
            "installed_systemd",
            "systemctl is not on PATH; cannot verify installed user services",
        );
        return;
    }

    for unit in SYSTEMD_SERVICE_UNITS {
        match read_systemd_installed_service_status(unit) {
            Ok(status) if status.is_ready() => push_service_doctor_check(
                checks,
                ServiceDoctorStatus::Ok,
                "installed_systemd",
                format!(
                    "installed systemd service {} is ready: {}",
                    unit,
                    status.summary()
                ),
            ),
            Ok(status) => push_service_doctor_check(
                checks,
                ServiceDoctorStatus::Blocker,
                "installed_systemd",
                format!(
                    "installed systemd service {} is not ready: {}",
                    unit,
                    status.summary()
                ),
            ),
            Err(error) => push_service_doctor_check(
                checks,
                ServiceDoctorStatus::Blocker,
                "installed_systemd",
                format!("failed to inspect installed systemd service {unit}: {error}"),
            ),
        }
    }
}

fn read_systemd_installed_service_status(unit: &str) -> AppResult<SystemdInstalledServiceStatus> {
    let output = Command::new("systemctl")
        .arg("--user")
        .arg("show")
        .arg(unit)
        .arg("--property=LoadState")
        .arg("--property=ActiveState")
        .arg("--property=SubState")
        .arg("--property=UnitFileState")
        .arg("--property=FragmentPath")
        .arg("--no-pager")
        .output()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(app_error(format!(
            "systemctl --user show failed with {}{}",
            output.status,
            child_stderr_suffix(stderr.trim())
        )));
    }
    parse_systemd_installed_service_status(unit, &stdout)
}

fn parse_systemd_installed_service_status(
    unit: &str,
    stdout: &str,
) -> AppResult<SystemdInstalledServiceStatus> {
    let mut values = BTreeMap::new();
    for line in stdout.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        values.insert(key.trim().to_string(), value.trim().to_string());
    }
    let load_state = values
        .get("LoadState")
        .cloned()
        .ok_or_else(|| app_error("systemd show output missing LoadState"))?;
    let active_state = values
        .get("ActiveState")
        .cloned()
        .ok_or_else(|| app_error("systemd show output missing ActiveState"))?;
    Ok(SystemdInstalledServiceStatus {
        unit: unit.to_string(),
        load_state,
        active_state,
        sub_state: values.get("SubState").cloned().unwrap_or_default(),
        unit_file_state: values.get("UnitFileState").cloned().unwrap_or_default(),
        fragment_path: values.get("FragmentPath").cloned().unwrap_or_default(),
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct LaunchdInstalledServiceStatus {
    label: String,
    state: String,
    pid: Option<String>,
    last_exit_status: Option<String>,
    path: Option<String>,
}

impl LaunchdInstalledServiceStatus {
    fn is_ready(&self) -> bool {
        let state_running = self.state == "running";
        let pid_running = self
            .pid
            .as_deref()
            .is_some_and(|pid| !pid.is_empty() && pid != "0" && pid != "-");
        let clean_exit = self
            .last_exit_status
            .as_deref()
            .map(|status| status == "0")
            .unwrap_or(true);
        (state_running || pid_running) && clean_exit
    }

    fn summary(&self) -> String {
        format!(
            "state={} pid={} last_exit_status={} path={}",
            service_status_value(&self.state),
            service_status_value(self.pid.as_deref().unwrap_or_default()),
            service_status_value(self.last_exit_status.as_deref().unwrap_or_default()),
            service_status_value(self.path.as_deref().unwrap_or_default())
        )
    }
}

fn service_doctor_check_installed_launchd_services(checks: &mut Vec<ServiceDoctorCheck>) {
    if !command_on_path("launchctl") {
        push_service_doctor_check(
            checks,
            ServiceDoctorStatus::Blocker,
            "installed_launchd",
            "launchctl is not on PATH; cannot verify installed user services",
        );
        return;
    }
    let uid = match current_user_id_string() {
        Ok(uid) => uid,
        Err(error) => {
            push_service_doctor_check(
                checks,
                ServiceDoctorStatus::Blocker,
                "installed_launchd",
                format!("failed to resolve current user id for launchctl print: {error}"),
            );
            return;
        }
    };

    for label in LAUNCHD_SERVICE_LABELS {
        match read_launchd_installed_service_status(&uid, label) {
            Ok(status) if status.is_ready() => push_service_doctor_check(
                checks,
                ServiceDoctorStatus::Ok,
                "installed_launchd",
                format!(
                    "installed launchd service {} is ready: {}",
                    label,
                    status.summary()
                ),
            ),
            Ok(status) => push_service_doctor_check(
                checks,
                ServiceDoctorStatus::Blocker,
                "installed_launchd",
                format!(
                    "installed launchd service {} is not ready: {}",
                    label,
                    status.summary()
                ),
            ),
            Err(error) => push_service_doctor_check(
                checks,
                ServiceDoctorStatus::Blocker,
                "installed_launchd",
                format!("failed to inspect installed launchd service {label}: {error}"),
            ),
        }
    }
}

fn current_user_id_string() -> AppResult<String> {
    if let Ok(uid) = std::env::var("UID") {
        let uid = uid.trim();
        if !uid.is_empty() {
            return Ok(uid.to_string());
        }
    }
    let output = Command::new("id").arg("-u").output()?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(app_error(format!(
            "id -u failed with {}{}",
            output.status,
            child_stderr_suffix(stderr.trim())
        )));
    }
    let uid = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if uid.is_empty() {
        return Err(app_error("id -u returned an empty user id"));
    }
    Ok(uid)
}

fn read_launchd_installed_service_status(
    uid: &str,
    label: &str,
) -> AppResult<LaunchdInstalledServiceStatus> {
    let target = format!("gui/{uid}/{label}");
    let output = Command::new("launchctl")
        .arg("print")
        .arg(&target)
        .output()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(app_error(format!(
            "launchctl print {target} failed with {}{}",
            output.status,
            child_stderr_suffix(stderr.trim())
        )));
    }
    parse_launchd_installed_service_status(label, &stdout)
}

fn parse_launchd_installed_service_status(
    label: &str,
    stdout: &str,
) -> AppResult<LaunchdInstalledServiceStatus> {
    let mut state = None;
    let mut pid = None;
    let mut last_exit_status = None;
    let mut path = None;

    for line in stdout.lines() {
        let Some((key, value)) = parse_launchd_status_line(line) else {
            continue;
        };
        match key.as_str() {
            "state" => state = Some(value),
            "pid" | "PID" => pid = Some(value),
            "last exit code" | "last_exit_status" | "LastExitStatus" => {
                last_exit_status = Some(value)
            }
            "path" => path = Some(value),
            _ => {}
        }
    }

    Ok(LaunchdInstalledServiceStatus {
        label: label.to_string(),
        state: state.ok_or_else(|| app_error("launchctl print output missing state"))?,
        pid,
        last_exit_status,
        path,
    })
}

fn parse_launchd_status_line(line: &str) -> Option<(String, String)> {
    let trimmed = line.trim().trim_end_matches(';');
    let (key, value) = trimmed
        .split_once(" = ")
        .or_else(|| trimmed.split_once('='))?;
    let key = key.trim().trim_matches('"').trim_matches('\'').to_string();
    let value = value
        .trim()
        .trim_end_matches(';')
        .trim_matches('"')
        .trim_matches('\'')
        .to_string();
    if key.is_empty() {
        return None;
    }
    Some((key, value))
}

fn service_status_value(value: &str) -> &str {
    if value.is_empty() {
        "-"
    } else {
        value
    }
}

fn service_doctor_check_output_dir(
    config: &ServiceTemplateConfig,
    templates: &[ServiceTemplate],
    checks: &mut Vec<ServiceDoctorCheck>,
) {
    let Some(out) = config.out.as_ref() else {
        push_service_doctor_check(
            checks,
            ServiceDoctorStatus::Ok,
            "output",
            "no --out directory supplied; on-disk template comparison skipped",
        );
        return;
    };
    if !out.is_dir() {
        push_service_doctor_check(
            checks,
            ServiceDoctorStatus::Blocker,
            "output",
            format!("service output directory is missing: {}", out.display()),
        );
        return;
    }

    for template in templates {
        let path = out.join(template.path);
        match std::fs::read_to_string(&path) {
            Ok(content) if content == template.body => push_service_doctor_check(
                checks,
                ServiceDoctorStatus::Ok,
                "output",
                format!(
                    "generated template matches current render: {}",
                    path.display()
                ),
            ),
            Ok(_) => push_service_doctor_check(
                checks,
                ServiceDoctorStatus::Blocker,
                "output",
                format!("generated template is stale or differs: {}", path.display()),
            ),
            Err(error) => push_service_doctor_check(
                checks,
                ServiceDoctorStatus::Blocker,
                "output",
                format!(
                    "generated template is missing or unreadable: {}: {error}",
                    path.display()
                ),
            ),
        }
    }

    let guide_path = out.join("SERVICES.md");
    match std::fs::read_to_string(&guide_path) {
        Ok(content)
            if content.contains("DeepSeekCode Service Lifecycle")
                && content.contains(&config.bin)
                && content.contains(&config.workdir) =>
        {
            push_service_doctor_check(
                checks,
                ServiceDoctorStatus::Ok,
                "output",
                format!(
                    "SERVICES.md matches target metadata: {}",
                    guide_path.display()
                ),
            );
        }
        Ok(_) => push_service_doctor_check(
            checks,
            ServiceDoctorStatus::Blocker,
            "output",
            format!(
                "SERVICES.md is stale or missing target metadata: {}",
                guide_path.display()
            ),
        ),
        Err(error) => push_service_doctor_check(
            checks,
            ServiceDoctorStatus::Blocker,
            "output",
            format!(
                "SERVICES.md is missing or unreadable: {}: {error}",
                guide_path.display()
            ),
        ),
    }
}

fn push_service_doctor_check(
    checks: &mut Vec<ServiceDoctorCheck>,
    status: ServiceDoctorStatus,
    name: impl Into<String>,
    message: impl Into<String>,
) {
    checks.push(ServiceDoctorCheck {
        status,
        name: name.into(),
        message: message.into(),
    });
}

fn expected_service_template_paths(kind: AgentsServiceKind) -> Vec<&'static str> {
    let mut paths = Vec::new();
    if matches!(kind, AgentsServiceKind::Systemd | AgentsServiceKind::All) {
        paths.extend([
            "systemd/deepseek-runtime.service",
            "systemd/deepseek-agents.service",
            "systemd/deepseek-diagnostics.service",
            "systemd/deepseek-shell-supervisor.service",
        ]);
    }
    if matches!(kind, AgentsServiceKind::Launchd | AgentsServiceKind::All) {
        paths.extend([
            "launchd/com.deepseek.runtime.plist",
            "launchd/com.deepseek.agents.plist",
            "launchd/com.deepseek.diagnostics.plist",
            "launchd/com.deepseek.shell-supervisor.plist",
        ]);
    }
    paths
}

fn service_value_is_path(value: &str) -> bool {
    value.contains('/') || value.contains('\\')
}

fn command_on_path(command: &str) -> bool {
    if command.trim().is_empty() || service_value_is_path(command) {
        return false;
    }
    let Some(paths) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&paths).any(|dir| {
        let candidate = dir.join(command);
        if candidate.is_file() {
            return true;
        }
        #[cfg(windows)]
        {
            let exe = dir.join(format!("{command}.exe"));
            if exe.is_file() {
                return true;
            }
        }
        false
    })
}

fn render_service_doctor_text(report: &ServiceDoctorReport) -> String {
    let mut out = String::new();
    out.push_str("DeepSeekCode service doctor\n");
    out.push_str(&format!(
        "  kind: {}\n",
        service_kind_label(report.config.kind)
    ));
    out.push_str(&format!("  binary: {}\n", report.config.bin));
    out.push_str(&format!("  workdir: {}\n", report.config.workdir));
    out.push_str(&format!("  runtime_addr: {}\n", report.config.addr));
    if let Some(out_dir) = &report.config.out {
        out.push_str(&format!("  output: {}\n", out_dir.display()));
    }
    out.push_str(&format!("  installed: {}\n", report.installed));
    out.push('\n');
    for check in &report.checks {
        out.push_str(&format!(
            "[{}] {}: {}\n",
            service_doctor_status_label(check.status),
            check.name,
            check.message
        ));
    }
    out.push_str(&format!(
        "\nsummary: {} blocker(s), {} warning(s)\n",
        report.blocker_count(),
        report.warning_count()
    ));
    out
}

fn render_service_doctor_json(report: &ServiceDoctorReport) -> String {
    let out = report
        .config
        .out
        .as_ref()
        .map(|path| path.display().to_string())
        .unwrap_or_default();
    let checks = report
        .checks
        .iter()
        .map(|check| {
            format!(
                "{{\"status\":\"{}\",\"name\":\"{}\",\"message\":\"{}\"}}",
                service_doctor_status_label(check.status),
                json_escape(&check.name),
                json_escape(&check.message)
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "{{\"kind\":\"deepseek.agents.service_doctor.v1\",\"service_kind\":\"{}\",\"binary\":\"{}\",\"workdir\":\"{}\",\"runtime_addr\":\"{}\",\"output_dir\":\"{}\",\"installed\":{},\"blockers\":{},\"warnings\":{},\"checks\":[{}]}}",
        service_kind_label(report.config.kind),
        json_escape(&report.config.bin),
        json_escape(&report.config.workdir),
        json_escape(&report.config.addr),
        json_escape(&out),
        report.installed,
        report.blocker_count(),
        report.warning_count(),
        checks
    )
}

fn service_kind_label(kind: AgentsServiceKind) -> &'static str {
    match kind {
        AgentsServiceKind::Systemd => "systemd",
        AgentsServiceKind::Launchd => "launchd",
        AgentsServiceKind::All => "all",
    }
}

fn service_doctor_status_label(status: ServiceDoctorStatus) -> &'static str {
    match status {
        ServiceDoctorStatus::Ok => "ok",
        ServiceDoctorStatus::Warn => "warn",
        ServiceDoctorStatus::Blocker => "blocker",
    }
}

#[derive(Debug, Clone)]
struct ServiceSmokeReport {
    kind: AgentsServiceKind,
    installed: bool,
    binary: String,
    workdir: PathBuf,
    requested_addr: String,
    resolved_addr: String,
    addr_error: Option<String>,
    timeout_ms: u64,
    checks: Vec<ServiceDoctorCheck>,
}

fn run_service_smoke(args: AgentsServiceSmokeArgs) -> AppResult<()> {
    let json = args.json;
    let mut report = build_service_smoke_report(args);
    run_service_smoke_checks(&mut report);
    if json {
        println!("{}", render_service_smoke_json(&report));
    } else {
        print!("{}", render_service_smoke_text(&report));
    }
    if service_check_blocker_count(&report.checks) > 0 {
        return Err(app_error(format!(
            "service smoke found {} blocker(s)",
            service_check_blocker_count(&report.checks)
        )));
    }
    Ok(())
}

fn build_service_smoke_report(args: AgentsServiceSmokeArgs) -> ServiceSmokeReport {
    let binary = resolve_service_smoke_binary(args.bin);
    let workdir = args
        .workdir
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
    let requested_addr = args.addr;
    let (resolved_addr, addr_error) =
        match resolve_service_smoke_addr_for_mode(&requested_addr, args.installed) {
            Ok(addr) => (addr, None),
            Err(error) => (requested_addr.clone(), Some(error.to_string())),
        };
    ServiceSmokeReport {
        kind: args.kind,
        installed: args.installed,
        binary,
        workdir,
        requested_addr,
        resolved_addr,
        addr_error,
        timeout_ms: args.timeout_ms.max(100),
        checks: Vec::new(),
    }
}

fn run_service_smoke_checks(report: &mut ServiceSmokeReport) {
    service_smoke_check_binary(report);
    service_smoke_check_workdir(report);
    service_smoke_check_addr(report);
    if service_check_blocker_count(&report.checks) == 0 {
        if report.installed {
            service_smoke_check_installed_services(report);
            service_smoke_check_installed_runtime(report);
            service_smoke_check_installed_shell_supervisor(report);
        } else {
            service_smoke_check_runtime(report);
            service_smoke_check_shell_supervisor(report);
        }
    }
}

fn service_smoke_check_binary(report: &mut ServiceSmokeReport) {
    if service_value_is_path(&report.binary) {
        if Path::new(&report.binary).is_file() {
            push_service_doctor_check(
                &mut report.checks,
                ServiceDoctorStatus::Ok,
                "binary",
                format!("binary path exists: {}", report.binary),
            );
        } else {
            push_service_doctor_check(
                &mut report.checks,
                ServiceDoctorStatus::Blocker,
                "binary",
                format!("binary path is missing or not a file: {}", report.binary),
            );
        }
    } else if command_on_path(&report.binary) {
        push_service_doctor_check(
            &mut report.checks,
            ServiceDoctorStatus::Ok,
            "binary",
            format!("binary command is on PATH: {}", report.binary),
        );
    } else {
        push_service_doctor_check(
            &mut report.checks,
            ServiceDoctorStatus::Blocker,
            "binary",
            format!("binary command is not on PATH: {}", report.binary),
        );
    }
}

fn service_smoke_check_workdir(report: &mut ServiceSmokeReport) {
    if report.workdir.is_dir() {
        push_service_doctor_check(
            &mut report.checks,
            ServiceDoctorStatus::Ok,
            "workdir",
            format!("workspace directory exists: {}", report.workdir.display()),
        );
    } else {
        push_service_doctor_check(
            &mut report.checks,
            ServiceDoctorStatus::Blocker,
            "workdir",
            format!(
                "workspace directory is missing or not a directory: {}",
                report.workdir.display()
            ),
        );
    }
}

fn service_smoke_check_addr(report: &mut ServiceSmokeReport) {
    if let Some(error) = &report.addr_error {
        push_service_doctor_check(
            &mut report.checks,
            ServiceDoctorStatus::Blocker,
            "runtime_addr",
            format!(
                "failed to resolve service smoke address {}: {error}",
                report.requested_addr
            ),
        );
    } else {
        push_service_doctor_check(
            &mut report.checks,
            ServiceDoctorStatus::Ok,
            "runtime_addr",
            format!(
                "runtime smoke address resolved: {} -> {}",
                report.requested_addr, report.resolved_addr
            ),
        );
    }
}

fn service_smoke_check_installed_services(report: &mut ServiceSmokeReport) {
    let before = report.checks.len();
    service_doctor_check_installed_services(report.kind, &mut report.checks);
    if report.checks.len() == before {
        push_service_doctor_check(
            &mut report.checks,
            ServiceDoctorStatus::Warn,
            "installed_services",
            "no installed service manager checks were selected",
        );
    }
}

fn service_smoke_check_installed_runtime(report: &mut ServiceSmokeReport) {
    let timeout = Duration::from_millis(report.timeout_ms);
    match probe_http_health(&report.resolved_addr, timeout) {
        Ok(_) => push_service_doctor_check(
            &mut report.checks,
            ServiceDoctorStatus::Ok,
            "installed_runtime",
            format!(
                "installed HTTP runtime /health responded at {}",
                report.resolved_addr
            ),
        ),
        Err(error) => push_service_doctor_check(
            &mut report.checks,
            ServiceDoctorStatus::Blocker,
            "installed_runtime",
            format!("installed HTTP runtime /health smoke failed: {error}"),
        ),
    }
}

#[cfg(unix)]
fn service_smoke_check_installed_shell_supervisor(report: &mut ServiceSmokeReport) {
    let timeout = Duration::from_millis(report.timeout_ms);
    let socket = service_smoke_shell_supervisor_socket_path(&report.workdir);
    if let Some(path_bytes) = unix_socket_path_too_long(&socket) {
        push_service_doctor_check(
            &mut report.checks,
            ServiceDoctorStatus::Blocker,
            "installed_shell_supervisor",
            format!(
                "installed shell supervisor socket path is too long for Unix domain sockets: {} ({} bytes; limit is < {} bytes)",
                socket.display(),
                path_bytes,
                SHELL_SUPERVISOR_UNIX_SOCKET_MAX_BYTES
            ),
        );
        return;
    }

    let supervisor_healthy = match wait_for_installed_shell_supervisor_health(&socket, timeout) {
        Ok(_) => {
            push_service_doctor_check(
                &mut report.checks,
                ServiceDoctorStatus::Ok,
                "installed_shell_supervisor",
                format!(
                    "installed shell supervisor health responded at {}",
                    socket.display()
                ),
            );
            true
        }
        Err(error) => {
            push_service_doctor_check(
                &mut report.checks,
                ServiceDoctorStatus::Blocker,
                "installed_shell_supervisor",
                format!("installed shell supervisor health smoke failed: {error}"),
            );
            false
        }
    };

    if supervisor_healthy {
        match shell_supervisor_control_smoke(&socket, report.timeout_ms, None) {
            Ok(summary) => push_service_doctor_check(
                &mut report.checks,
                ServiceDoctorStatus::Ok,
                "installed_shell_supervisor_control",
                summary,
            ),
            Err(error) => push_service_doctor_check(
                &mut report.checks,
                ServiceDoctorStatus::Blocker,
                "installed_shell_supervisor_control",
                format!("installed shell supervisor control smoke failed: {error}"),
            ),
        }
    }
}

#[cfg(windows)]
fn service_smoke_check_installed_shell_supervisor(report: &mut ServiceSmokeReport) {
    match wait_for_installed_shell_supervisor_tcp_health(
        &report.workdir,
        Duration::from_millis(report.timeout_ms),
    ) {
        Ok(endpoint) => {
            push_service_doctor_check(
                &mut report.checks,
                ServiceDoctorStatus::Ok,
                "installed_shell_supervisor",
                format!("installed shell supervisor health responded at tcp://{endpoint}"),
            );
            match shell_supervisor_windows_tcp_control_smoke(&endpoint, report.timeout_ms) {
                Ok(summary) => push_service_doctor_check(
                    &mut report.checks,
                    ServiceDoctorStatus::Ok,
                    "installed_shell_supervisor_control",
                    summary,
                ),
                Err(error) => push_service_doctor_check(
                    &mut report.checks,
                    ServiceDoctorStatus::Blocker,
                    "installed_shell_supervisor_control",
                    format!("installed shell supervisor control smoke failed: {error}"),
                ),
            }
        }
        Err(error) => push_service_doctor_check(
            &mut report.checks,
            ServiceDoctorStatus::Blocker,
            "installed_shell_supervisor",
            format!("installed shell supervisor TCP health smoke failed: {error}"),
        ),
    }
}

#[cfg(not(any(unix, windows)))]
fn service_smoke_check_installed_shell_supervisor(report: &mut ServiceSmokeReport) {
    push_service_doctor_check(
        &mut report.checks,
        ServiceDoctorStatus::Warn,
        "installed_shell_supervisor",
        "installed shell supervisor smoke is only available on Unix and Windows",
    );
}

fn service_smoke_check_runtime(report: &mut ServiceSmokeReport) {
    let timeout = Duration::from_millis(report.timeout_ms);
    let mut child = match Command::new(&report.binary)
        .arg("serve")
        .arg("--http")
        .arg("--addr")
        .arg(&report.resolved_addr)
        .arg("--once")
        .current_dir(&report.workdir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(error) => {
            push_service_doctor_check(
                &mut report.checks,
                ServiceDoctorStatus::Blocker,
                "runtime",
                format!("failed to start HTTP runtime smoke child: {error}"),
            );
            return;
        }
    };

    match wait_for_http_health(&report.resolved_addr, timeout, &mut child) {
        Ok(_) => push_service_doctor_check(
            &mut report.checks,
            ServiceDoctorStatus::Ok,
            "runtime",
            format!("HTTP runtime /health responded at {}", report.resolved_addr),
        ),
        Err(error) => push_service_doctor_check(
            &mut report.checks,
            ServiceDoctorStatus::Blocker,
            "runtime",
            format!("HTTP runtime /health smoke failed: {error}"),
        ),
    }

    match wait_child_exit(&mut child, timeout) {
        Ok(status) if status.success() => push_service_doctor_check(
            &mut report.checks,
            ServiceDoctorStatus::Ok,
            "runtime",
            format!("HTTP runtime smoke child exited successfully: {status}"),
        ),
        Ok(status) => {
            let stderr = read_child_stderr(&mut child);
            push_service_doctor_check(
                &mut report.checks,
                ServiceDoctorStatus::Blocker,
                "runtime",
                format!(
                    "HTTP runtime smoke child exited with failure: {status}{}",
                    child_stderr_suffix(&stderr)
                ),
            );
        }
        Err(error) => {
            terminate_child(&mut child);
            push_service_doctor_check(
                &mut report.checks,
                ServiceDoctorStatus::Blocker,
                "runtime",
                format!("HTTP runtime smoke child did not exit cleanly: {error}"),
            );
        }
    }
}

#[cfg(unix)]
fn service_smoke_check_shell_supervisor(report: &mut ServiceSmokeReport) {
    let timeout = Duration::from_millis(report.timeout_ms);
    let socket = service_smoke_shell_supervisor_socket_path(&report.workdir);
    if let Some(path_bytes) = unix_socket_path_too_long(&socket) {
        push_service_doctor_check(
            &mut report.checks,
            ServiceDoctorStatus::Blocker,
            "shell_supervisor",
            format!(
                "shell supervisor socket path is too long for Unix domain sockets: {} ({} bytes; limit is < {} bytes). Rerun with a shorter --workdir such as /tmp/dsc-smk",
                socket.display(),
                path_bytes,
                SHELL_SUPERVISOR_UNIX_SOCKET_MAX_BYTES
            ),
        );
        return;
    }
    if shell_supervisor_socket_is_active(&socket) {
        push_service_doctor_check(
            &mut report.checks,
            ServiceDoctorStatus::Blocker,
            "shell_supervisor",
            format!(
                "shell supervisor socket is already active at {}; rerun with an isolated --workdir",
                socket.display()
            ),
        );
        return;
    }

    let mut child = match Command::new(&report.binary)
        .arg("agents")
        .arg("shell-supervisor")
        .arg("--json")
        .current_dir(&report.workdir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(error) => {
            push_service_doctor_check(
                &mut report.checks,
                ServiceDoctorStatus::Blocker,
                "shell_supervisor",
                format!("failed to start shell supervisor smoke child: {error}"),
            );
            return;
        }
    };

    let supervisor_healthy = match wait_for_shell_supervisor_health(&socket, timeout, &mut child) {
        Ok(_) => {
            push_service_doctor_check(
                &mut report.checks,
                ServiceDoctorStatus::Ok,
                "shell_supervisor",
                format!("shell supervisor health responded at {}", socket.display()),
            );
            true
        }
        Err(error) => {
            push_service_doctor_check(
                &mut report.checks,
                ServiceDoctorStatus::Blocker,
                "shell_supervisor",
                format!("shell supervisor health smoke failed: {error}"),
            );
            false
        }
    };

    if supervisor_healthy {
        match shell_supervisor_control_smoke(
            &socket,
            report.timeout_ms,
            Some((&report.binary, report.workdir.as_path())),
        ) {
            Ok(summary) => push_service_doctor_check(
                &mut report.checks,
                ServiceDoctorStatus::Ok,
                "shell_supervisor_control",
                summary,
            ),
            Err(error) => push_service_doctor_check(
                &mut report.checks,
                ServiceDoctorStatus::Blocker,
                "shell_supervisor_control",
                format!("shell supervisor control smoke failed: {error}"),
            ),
        }
    }

    match shell_supervisor_request(&socket, "shutdown") {
        Ok(_) => {}
        Err(error) => push_service_doctor_check(
            &mut report.checks,
            ServiceDoctorStatus::Warn,
            "shell_supervisor",
            format!("shell supervisor shutdown request failed: {error}"),
        ),
    }

    match wait_child_exit(&mut child, timeout) {
        Ok(status) if status.success() => push_service_doctor_check(
            &mut report.checks,
            ServiceDoctorStatus::Ok,
            "shell_supervisor",
            format!("shell supervisor smoke child exited successfully: {status}"),
        ),
        Ok(status) => {
            let stderr = read_child_stderr(&mut child);
            push_service_doctor_check(
                &mut report.checks,
                ServiceDoctorStatus::Blocker,
                "shell_supervisor",
                format!(
                    "shell supervisor smoke child exited with failure: {status}{}",
                    child_stderr_suffix(&stderr)
                ),
            );
        }
        Err(error) => {
            terminate_child(&mut child);
            push_service_doctor_check(
                &mut report.checks,
                ServiceDoctorStatus::Blocker,
                "shell_supervisor",
                format!("shell supervisor smoke child did not exit cleanly: {error}"),
            );
        }
    }
}

#[cfg(windows)]
fn service_smoke_check_shell_supervisor(report: &mut ServiceSmokeReport) {
    let timeout = Duration::from_millis(report.timeout_ms);
    let mut child = match Command::new(&report.binary)
        .arg("agents")
        .arg("shell-supervisor")
        .arg("--json")
        .current_dir(&report.workdir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(error) => {
            push_service_doctor_check(
                &mut report.checks,
                ServiceDoctorStatus::Blocker,
                "shell_supervisor",
                format!("failed to start shell supervisor smoke child: {error}"),
            );
            return;
        }
    };

    let endpoint = match wait_for_shell_supervisor_tcp_health(&report.workdir, timeout, &mut child)
    {
        Ok(endpoint) => {
            push_service_doctor_check(
                &mut report.checks,
                ServiceDoctorStatus::Ok,
                "shell_supervisor",
                format!("shell supervisor health responded at tcp://{endpoint}"),
            );
            Some(endpoint)
        }
        Err(error) => {
            push_service_doctor_check(
                &mut report.checks,
                ServiceDoctorStatus::Blocker,
                "shell_supervisor",
                format!("shell supervisor health smoke failed: {error}"),
            );
            None
        }
    };

    if let Some(endpoint) = endpoint.as_deref() {
        match shell_supervisor_windows_tcp_control_smoke(endpoint, report.timeout_ms) {
            Ok(summary) => push_service_doctor_check(
                &mut report.checks,
                ServiceDoctorStatus::Ok,
                "shell_supervisor_control",
                summary,
            ),
            Err(error) => push_service_doctor_check(
                &mut report.checks,
                ServiceDoctorStatus::Blocker,
                "shell_supervisor_control",
                format!("shell supervisor control smoke failed: {error}"),
            ),
        }
        match shell_supervisor_tcp_request(endpoint, "shutdown") {
            Ok(_) => {}
            Err(error) => push_service_doctor_check(
                &mut report.checks,
                ServiceDoctorStatus::Warn,
                "shell_supervisor",
                format!("shell supervisor shutdown request failed: {error}"),
            ),
        }
    }

    match wait_child_exit(&mut child, timeout) {
        Ok(status) if status.success() => push_service_doctor_check(
            &mut report.checks,
            ServiceDoctorStatus::Ok,
            "shell_supervisor",
            format!("shell supervisor smoke child exited successfully: {status}"),
        ),
        Ok(status) => {
            let stderr = read_child_stderr(&mut child);
            push_service_doctor_check(
                &mut report.checks,
                ServiceDoctorStatus::Blocker,
                "shell_supervisor",
                format!(
                    "shell supervisor smoke child exited with failure: {status}{}",
                    child_stderr_suffix(&stderr)
                ),
            );
        }
        Err(error) => {
            terminate_child(&mut child);
            push_service_doctor_check(
                &mut report.checks,
                ServiceDoctorStatus::Blocker,
                "shell_supervisor",
                format!("shell supervisor smoke child did not exit cleanly: {error}"),
            );
        }
    }
}

#[cfg(not(any(unix, windows)))]
fn service_smoke_check_shell_supervisor(report: &mut ServiceSmokeReport) {
    push_service_doctor_check(
        &mut report.checks,
        ServiceDoctorStatus::Warn,
        "shell_supervisor",
        "shell supervisor smoke is only available on Unix and Windows",
    );
}

#[derive(Debug, Clone)]
struct ShellFixtureSmokeReport {
    binary: String,
    workdir: PathBuf,
    checks: Vec<ServiceDoctorCheck>,
}

fn run_shell_fixture_smoke(json: bool) -> AppResult<()> {
    let mut report = build_shell_fixture_smoke_report();
    run_shell_fixture_smoke_checks(&mut report);
    if json {
        println!("{}", render_shell_fixture_smoke_json(&report));
    } else {
        print!("{}", render_shell_fixture_smoke_text(&report));
    }
    if service_check_blocker_count(&report.checks) > 0 {
        return Err(app_error(format!(
            "shell fixture smoke found {} blocker(s)",
            service_check_blocker_count(&report.checks)
        )));
    }
    Ok(())
}

fn build_shell_fixture_smoke_report() -> ShellFixtureSmokeReport {
    let binary = std::env::current_exe()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|_| "deepseek".to_string());
    ShellFixtureSmokeReport {
        binary,
        workdir: temp_shell_fixture_smoke_root(),
        checks: Vec::new(),
    }
}

fn temp_shell_fixture_smoke_root() -> PathBuf {
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        % 100_000;
    short_temp_root().join(format!("dsc-shell-fixture-{}-{suffix}", std::process::id()))
}

fn short_temp_root() -> PathBuf {
    let tmp = PathBuf::from("/tmp");
    if tmp.is_dir() {
        return tmp;
    }
    std::env::temp_dir()
}

fn run_shell_fixture_smoke_checks(report: &mut ShellFixtureSmokeReport) {
    match std::fs::create_dir_all(&report.workdir) {
        Ok(_) => push_service_doctor_check(
            &mut report.checks,
            ServiceDoctorStatus::Ok,
            "workdir",
            format!("created fixture workspace: {}", report.workdir.display()),
        ),
        Err(error) => {
            push_service_doctor_check(
                &mut report.checks,
                ServiceDoctorStatus::Blocker,
                "workdir",
                format!(
                    "failed to create fixture workspace {}: {error}",
                    report.workdir.display()
                ),
            );
            return;
        }
    }

    #[cfg(unix)]
    run_shell_fixture_smoke_unix(report);
    #[cfg(windows)]
    run_shell_fixture_smoke_windows(report);
    #[cfg(not(any(unix, windows)))]
    push_service_doctor_check(
        &mut report.checks,
        ServiceDoctorStatus::Blocker,
        "platform",
        "shell fixture smoke requires Unix or Windows shell-supervisor IPC",
    );

    let _ = std::fs::remove_dir_all(&report.workdir);
}

#[cfg(unix)]
fn run_shell_fixture_smoke_unix(report: &mut ShellFixtureSmokeReport) {
    let timeout = Duration::from_millis(5_000);
    let socket = service_smoke_shell_supervisor_socket_path(&report.workdir);
    if let Some(path_bytes) = unix_socket_path_too_long(&socket) {
        push_service_doctor_check(
            &mut report.checks,
            ServiceDoctorStatus::Blocker,
            "socket",
            format!(
                "shell fixture socket path is too long: {} ({} bytes; limit is < {} bytes)",
                socket.display(),
                path_bytes,
                SHELL_SUPERVISOR_UNIX_SOCKET_MAX_BYTES
            ),
        );
        return;
    }

    let mut child = match Command::new(&report.binary)
        .arg("agents")
        .arg("shell-supervisor")
        .arg("--json")
        .current_dir(&report.workdir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(error) => {
            push_service_doctor_check(
                &mut report.checks,
                ServiceDoctorStatus::Blocker,
                "shell_supervisor",
                format!("failed to start shell supervisor fixture child: {error}"),
            );
            return;
        }
    };

    let healthy = match wait_for_shell_supervisor_health(&socket, timeout, &mut child) {
        Ok(_) => {
            push_service_doctor_check(
                &mut report.checks,
                ServiceDoctorStatus::Ok,
                "health",
                format!("shell supervisor health responded at {}", socket.display()),
            );
            true
        }
        Err(error) => {
            push_service_doctor_check(
                &mut report.checks,
                ServiceDoctorStatus::Blocker,
                "health",
                format!("shell supervisor fixture health failed: {error}"),
            );
            false
        }
    };

    if healthy {
        match shell_supervisor_control_smoke(
            &socket,
            5_000,
            Some((&report.binary, report.workdir.as_path())),
        ) {
            Ok(summary) => push_service_doctor_check(
                &mut report.checks,
                ServiceDoctorStatus::Ok,
                "shell_control",
                summary,
            ),
            Err(error) => push_service_doctor_check(
                &mut report.checks,
                ServiceDoctorStatus::Blocker,
                "shell_control",
                format!("shell supervisor fixture control failed: {error}"),
            ),
        }
    }

    let _ = shell_supervisor_request(&socket, "shutdown");
    match wait_child_exit(&mut child, timeout) {
        Ok(status) if status.success() => push_service_doctor_check(
            &mut report.checks,
            ServiceDoctorStatus::Ok,
            "shutdown",
            format!("shell supervisor fixture child exited successfully: {status}"),
        ),
        Ok(status) => {
            let stderr = read_child_stderr(&mut child);
            push_service_doctor_check(
                &mut report.checks,
                ServiceDoctorStatus::Blocker,
                "shutdown",
                format!(
                    "shell supervisor fixture child exited with failure: {status}{}",
                    child_stderr_suffix(&stderr)
                ),
            );
        }
        Err(error) => {
            terminate_child(&mut child);
            push_service_doctor_check(
                &mut report.checks,
                ServiceDoctorStatus::Blocker,
                "shutdown",
                format!("shell supervisor fixture child did not exit cleanly: {error}"),
            );
        }
    }
}

#[cfg(windows)]
fn run_shell_fixture_smoke_windows(report: &mut ShellFixtureSmokeReport) {
    let timeout = Duration::from_millis(5_000);
    let mut child = match Command::new(&report.binary)
        .arg("agents")
        .arg("shell-supervisor")
        .arg("--json")
        .current_dir(&report.workdir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(error) => {
            push_service_doctor_check(
                &mut report.checks,
                ServiceDoctorStatus::Blocker,
                "shell_supervisor",
                format!("failed to start shell supervisor fixture child: {error}"),
            );
            return;
        }
    };

    let endpoint = match wait_for_shell_supervisor_tcp_health(&report.workdir, timeout, &mut child)
    {
        Ok(endpoint) => {
            push_service_doctor_check(
                &mut report.checks,
                ServiceDoctorStatus::Ok,
                "health",
                format!("shell supervisor health responded at tcp://{endpoint}"),
            );
            Some(endpoint)
        }
        Err(error) => {
            push_service_doctor_check(
                &mut report.checks,
                ServiceDoctorStatus::Blocker,
                "health",
                format!("shell supervisor fixture health failed: {error}"),
            );
            None
        }
    };

    if let Some(endpoint) = endpoint.as_deref() {
        match shell_supervisor_windows_tcp_control_smoke(endpoint, 5_000) {
            Ok(summary) => push_service_doctor_check(
                &mut report.checks,
                ServiceDoctorStatus::Ok,
                "shell_control",
                summary,
            ),
            Err(error) => push_service_doctor_check(
                &mut report.checks,
                ServiceDoctorStatus::Blocker,
                "shell_control",
                format!("shell supervisor fixture control failed: {error}"),
            ),
        }
        let _ = shell_supervisor_tcp_request(endpoint, "shutdown");
    }

    match wait_child_exit(&mut child, timeout) {
        Ok(status) if status.success() => push_service_doctor_check(
            &mut report.checks,
            ServiceDoctorStatus::Ok,
            "shutdown",
            format!("shell supervisor fixture child exited successfully: {status}"),
        ),
        Ok(status) => {
            let stderr = read_child_stderr(&mut child);
            push_service_doctor_check(
                &mut report.checks,
                ServiceDoctorStatus::Blocker,
                "shutdown",
                format!(
                    "shell supervisor fixture child exited with failure: {status}{}",
                    child_stderr_suffix(&stderr)
                ),
            );
        }
        Err(error) => {
            terminate_child(&mut child);
            push_service_doctor_check(
                &mut report.checks,
                ServiceDoctorStatus::Blocker,
                "shutdown",
                format!("shell supervisor fixture child did not exit cleanly: {error}"),
            );
        }
    }
}

fn render_shell_fixture_smoke_text(report: &ShellFixtureSmokeReport) -> String {
    let mut out = String::new();
    out.push_str("DeepSeekCode shell fixture smoke\n");
    out.push_str(&format!("  binary: {}\n", report.binary));
    out.push_str(&format!("  workdir: {}\n\n", report.workdir.display()));
    for check in &report.checks {
        out.push_str(&format!(
            "[{}] {}: {}\n",
            service_doctor_status_label(check.status),
            check.name,
            check.message
        ));
    }
    out.push_str(&format!(
        "\nsummary: {} blocker(s), {} warning(s)\n",
        service_check_blocker_count(&report.checks),
        service_check_warning_count(&report.checks)
    ));
    out
}

fn render_shell_fixture_smoke_json(report: &ShellFixtureSmokeReport) -> String {
    let checks = report
        .checks
        .iter()
        .map(|check| {
            format!(
                "{{\"status\":\"{}\",\"name\":\"{}\",\"message\":\"{}\"}}",
                service_doctor_status_label(check.status),
                json_escape(&check.name),
                json_escape(&check.message)
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "{{\"kind\":\"deepseek.agents.shell_fixture_smoke.v1\",\"ok\":{},\"binary\":\"{}\",\"workdir\":\"{}\",\"blockers\":{},\"warnings\":{},\"checks\":[{}]}}",
        service_check_blocker_count(&report.checks) == 0,
        json_escape(&report.binary),
        json_escape(&report.workdir.display().to_string()),
        service_check_blocker_count(&report.checks),
        service_check_warning_count(&report.checks),
        checks
    )
}

fn resolve_service_smoke_binary(bin: Option<String>) -> String {
    let Some(bin) = bin else {
        return std::env::current_exe()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|_| "deepseek".to_string());
    };
    let path = PathBuf::from(&bin);
    if service_value_is_path(&bin) && path.is_relative() {
        if let Ok(cwd) = std::env::current_dir() {
            return cwd.join(path).display().to_string();
        }
    }
    bin
}

fn resolve_service_smoke_addr_for_mode(addr: &str, installed: bool) -> AppResult<String> {
    if installed {
        let socket = addr.parse::<std::net::SocketAddr>().map_err(|error| {
            app_error(format!(
                "invalid installed service-smoke address `{addr}`: {error}"
            ))
        })?;
        if socket.port() == 0 {
            return Err(app_error(
                "installed service-smoke requires a concrete --addr port; 127.0.0.1:0 is only valid for non-installed smoke",
            ));
        }
        return Ok(socket.to_string());
    }
    resolve_service_smoke_addr(addr)
}

fn resolve_service_smoke_addr(addr: &str) -> AppResult<String> {
    use std::net::{SocketAddr, TcpListener};

    let socket = addr
        .parse::<SocketAddr>()
        .map_err(|error| app_error(format!("invalid service-smoke address `{addr}`: {error}")))?;
    if socket.port() != 0 {
        return Ok(socket.to_string());
    }
    let listener = TcpListener::bind(socket).map_err(|error| {
        app_error(format!(
            "failed to reserve service-smoke loopback address {addr}: {error}"
        ))
    })?;
    Ok(listener.local_addr()?.to_string())
}

fn probe_http_health(addr: &str, timeout: Duration) -> AppResult<String> {
    use std::net::TcpStream;

    let start = Instant::now();
    loop {
        match TcpStream::connect(addr) {
            Ok(stream) => return read_http_health_response(addr, stream),
            Err(error) if start.elapsed() < timeout => {
                let _ = error;
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(error) => {
                return Err(app_error(format!(
                    "timed out waiting for HTTP runtime at {addr}: {error}"
                )));
            }
        }
    }
}

fn read_http_health_response(addr: &str, mut stream: std::net::TcpStream) -> AppResult<String> {
    stream.set_read_timeout(Some(Duration::from_millis(500)))?;
    stream.set_write_timeout(Some(Duration::from_millis(500)))?;
    stream.write_all(b"GET /health HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")?;
    let mut response = String::new();
    stream.read_to_string(&mut response)?;
    if response.contains("200 OK") && response.contains("deepseek.runtime.health.v1") {
        return Ok(response);
    }
    Err(app_error(format!(
        "unexpected HTTP runtime /health response from {addr}"
    )))
}

fn wait_for_http_health(addr: &str, timeout: Duration, child: &mut Child) -> AppResult<String> {
    use std::net::TcpStream;

    let start = Instant::now();
    loop {
        if let Some(status) = child.try_wait()? {
            let stderr = read_child_stderr(child);
            return Err(app_error(format!(
                "HTTP runtime exited before /health responded: {status}{}",
                child_stderr_suffix(&stderr)
            )));
        }
        match TcpStream::connect(addr) {
            Ok(stream) => return read_http_health_response(addr, stream),
            Err(error) if start.elapsed() < timeout => {
                let _ = error;
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(error) => {
                return Err(app_error(format!(
                    "timed out waiting for HTTP runtime at {addr}: {error}"
                )));
            }
        }
    }
}

#[cfg(unix)]
fn wait_for_installed_shell_supervisor_health(
    socket: &Path,
    timeout: Duration,
) -> AppResult<String> {
    let start = Instant::now();
    loop {
        match shell_supervisor_request(socket, "health") {
            Ok(response) => return Ok(response),
            Err(error) if start.elapsed() < timeout => {
                let _ = error;
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(error) => {
                return Err(app_error(format!(
                    "timed out waiting for installed shell supervisor at {}: {error}",
                    socket.display()
                )));
            }
        }
    }
}

#[cfg(windows)]
fn wait_for_installed_shell_supervisor_tcp_health(
    workdir: &Path,
    timeout: Duration,
) -> AppResult<String> {
    let start = Instant::now();
    loop {
        match read_shell_supervisor_tcp_endpoint(workdir).and_then(|endpoint| {
            shell_supervisor_tcp_request(&endpoint, "health").map(|_| endpoint)
        }) {
            Ok(endpoint) => return Ok(endpoint),
            Err(error) if start.elapsed() < timeout => {
                let _ = error;
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(error) => {
                return Err(app_error(format!(
                    "timed out waiting for installed shell supervisor TCP endpoint: {error}"
                )));
            }
        }
    }
}

#[cfg(unix)]
fn wait_for_shell_supervisor_health(
    socket: &Path,
    timeout: Duration,
    child: &mut Child,
) -> AppResult<String> {
    let start = Instant::now();
    loop {
        if let Some(status) = child.try_wait()? {
            let stderr = read_child_stderr(child);
            return Err(app_error(format!(
                "shell supervisor exited before health responded: {status}{}",
                child_stderr_suffix(&stderr)
            )));
        }
        match shell_supervisor_request(socket, "health") {
            Ok(response) => return Ok(response),
            Err(error) if start.elapsed() < timeout => {
                let _ = error;
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(error) => {
                return Err(app_error(format!(
                    "timed out waiting for shell supervisor at {}: {error}",
                    socket.display()
                )));
            }
        }
    }
}

#[cfg(windows)]
fn wait_for_shell_supervisor_tcp_health(
    workdir: &Path,
    timeout: Duration,
    child: &mut Child,
) -> AppResult<String> {
    let start = Instant::now();
    loop {
        if let Some(status) = child.try_wait()? {
            let stderr = read_child_stderr(child);
            return Err(app_error(format!(
                "shell supervisor exited before TCP health responded: {status}{}",
                child_stderr_suffix(&stderr)
            )));
        }
        match read_shell_supervisor_tcp_endpoint(workdir).and_then(|endpoint| {
            shell_supervisor_tcp_request(&endpoint, "health").map(|_| endpoint)
        }) {
            Ok(endpoint) => return Ok(endpoint),
            Err(error) if start.elapsed() < timeout => {
                let _ = error;
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(error) => {
                return Err(app_error(format!(
                    "timed out waiting for shell supervisor TCP endpoint: {error}"
                )));
            }
        }
    }
}

#[cfg(unix)]
fn shell_supervisor_socket_is_active(socket: &Path) -> bool {
    shell_supervisor_request(socket, "health").is_ok()
}

#[cfg(all(unix, target_os = "linux"))]
const SHELL_SUPERVISOR_UNIX_SOCKET_MAX_BYTES: usize = 108;

#[cfg(all(unix, not(target_os = "linux")))]
const SHELL_SUPERVISOR_UNIX_SOCKET_MAX_BYTES: usize = 100;

#[cfg(unix)]
fn service_smoke_shell_supervisor_socket_path(workdir: &Path) -> PathBuf {
    let base = workdir.canonicalize().unwrap_or_else(|_| {
        if workdir.is_absolute() {
            workdir.to_path_buf()
        } else {
            std::env::current_dir()
                .map(|cwd| cwd.join(workdir))
                .unwrap_or_else(|_| workdir.to_path_buf())
        }
    });
    base.join(".dscode/shell-supervisor/supervisor.sock")
}

#[cfg(unix)]
fn unix_socket_path_too_long(path: &Path) -> Option<usize> {
    use std::os::unix::ffi::OsStrExt;

    let path_bytes = path.as_os_str().as_bytes().len();
    (path_bytes >= SHELL_SUPERVISOR_UNIX_SOCKET_MAX_BYTES).then_some(path_bytes)
}

#[cfg(unix)]
fn shell_supervisor_request(socket: &Path, method: &str) -> AppResult<String> {
    let request = format!("{{\"method\":\"{}\"}}\n", json_escape(method));
    shell_supervisor_request_raw(socket, method, &request)
}

#[cfg(unix)]
fn shell_supervisor_request_raw(socket: &Path, method: &str, request: &str) -> AppResult<String> {
    use std::os::unix::net::UnixStream;

    let mut stream = UnixStream::connect(socket)?;
    stream.set_read_timeout(Some(Duration::from_millis(5_000)))?;
    stream.set_write_timeout(Some(Duration::from_millis(1_000)))?;
    stream.write_all(request.as_bytes())?;
    let mut reader = BufReader::new(stream);
    let mut response = String::new();
    reader.read_line(&mut response)?;
    if response.contains("\"status\":\"ok\"")
        && response.contains(&format!("\"method\":\"{method}\""))
    {
        Ok(response)
    } else {
        Err(app_error(format!(
            "unexpected shell supervisor {method} response: {}",
            response.trim()
        )))
    }
}

#[cfg(windows)]
fn shell_supervisor_tcp_request(endpoint: &str, method: &str) -> AppResult<String> {
    let request = format!("{{\"method\":\"{}\"}}\n", json_escape(method));
    shell_supervisor_tcp_request_raw(endpoint, method, &request)
}

#[cfg(windows)]
fn shell_supervisor_tcp_request_raw(
    endpoint: &str,
    method: &str,
    request: &str,
) -> AppResult<String> {
    use std::net::{SocketAddr, TcpStream};

    let address = endpoint.parse::<SocketAddr>().map_err(|error| {
        app_error(format!(
            "invalid shell supervisor TCP endpoint tcp://{endpoint}: {error}"
        ))
    })?;
    if !address.ip().is_loopback() {
        return Err(app_error(format!(
            "shell supervisor TCP endpoint must be loopback: tcp://{endpoint}"
        )));
    }
    let mut stream = TcpStream::connect_timeout(&address, Duration::from_millis(5_000))?;
    stream.set_read_timeout(Some(Duration::from_millis(5_000)))?;
    stream.set_write_timeout(Some(Duration::from_millis(1_000)))?;
    stream.write_all(request.as_bytes())?;
    let mut reader = BufReader::new(stream);
    let mut response = String::new();
    reader.read_line(&mut response)?;
    if response.contains("\"status\":\"ok\"")
        && response.contains(&format!("\"method\":\"{method}\""))
    {
        Ok(response)
    } else {
        Err(app_error(format!(
            "unexpected shell supervisor {method} response: {}",
            response.trim()
        )))
    }
}

#[cfg(unix)]
fn shell_supervisor_control_smoke(
    socket: &Path,
    timeout_ms: u64,
    cli_smoke: Option<(&str, &Path)>,
) -> AppResult<String> {
    let tty = cfg!(all(unix, target_os = "linux"));
    let wait_timeout = timeout_ms.min(5000);
    let start_request = if tty {
        format!(
            "{{\"method\":\"start\",\"arguments\":{{\"command\":\"echo deepseek-shell-supervisor-smoke\",\"tty\":true,\"tty_rows\":24,\"tty_cols\":80,\"timeout_ms\":{wait_timeout}}}}}\n"
        )
    } else {
        format!(
            "{{\"method\":\"start\",\"arguments\":{{\"command\":\"echo deepseek-shell-supervisor-smoke\",\"tty\":false,\"timeout_ms\":{wait_timeout}}}}}\n"
        )
    };
    let start_response = shell_supervisor_request_raw(socket, "start", &start_request)?;
    let task_id = shell_supervisor_response_string(&start_response, "task_id")
        .ok_or_else(|| app_error("shell supervisor start smoke response missing task_id"))?;
    if tty && !start_response.contains(r#""job_pty_backend":"native-supervisor""#) {
        return Err(app_error(
            "shell supervisor tty smoke did not start a native-supervisor PTY job",
        ));
    }

    let wait_request = format!(
        "{{\"method\":\"wait\",\"arguments\":{{\"task_id\":\"{}\",\"timeout_ms\":{}}}}}\n",
        json_escape(&task_id),
        wait_timeout
    );
    shell_supervisor_request_raw(socket, "wait", &wait_request)?;

    let attach_request = format!(
        "{{\"method\":\"attach\",\"arguments\":{{\"task_id\":\"{}\",\"tail\":true,\"limit_bytes\":4096}}}}\n",
        json_escape(&task_id)
    );
    let attach_response = shell_supervisor_request_raw(socket, "attach", &attach_request)?;
    if !attach_response.contains("deepseek-shell-supervisor-smoke") {
        return Err(app_error(
            "shell supervisor attach smoke did not replay command output",
        ));
    }

    let replay_stream = if tty { "terminal" } else { "stdout" };
    let replay_request = format!(
        "{{\"method\":\"replay\",\"arguments\":{{\"task_id\":\"{}\",\"stream\":\"{}\",\"cursor\":0,\"limit_bytes\":4096}}}}\n",
        json_escape(&task_id),
        replay_stream
    );
    let replay_response = shell_supervisor_request_raw(socket, "replay", &replay_request)?;
    if !replay_response.contains("deepseek-shell-supervisor-smoke") {
        return Err(app_error(
            "shell supervisor replay smoke did not return command output",
        ));
    }

    let mut summaries = Vec::new();
    if tty {
        summaries.push(shell_supervisor_pty_control_smoke(socket, wait_timeout)?);
        summaries.push(shell_supervisor_byte_stream_proxy_smoke(
            socket,
            wait_timeout,
            cli_smoke,
        )?);
    }

    let mut summary = format!(
        "shell supervisor start/wait/attach/replay control smoke passed for task {task_id} (tty={tty})"
    );
    if !summaries.is_empty() {
        summary.push_str("; ");
        summary.push_str(&summaries.join("; "));
    }
    Ok(summary)
}

#[cfg(any(unix, windows))]
fn shell_supervisor_response_string(response: &str, key: &str) -> Option<String> {
    let value = parse_json_value(response.trim()).ok()?;
    let object = json_as_object(&value)?;
    json_as_string(object.get(key)?).map(str::to_string)
}

#[cfg(windows)]
fn shell_supervisor_windows_tcp_control_smoke(
    endpoint: &str,
    timeout_ms: u64,
) -> AppResult<String> {
    let wait_timeout = timeout_ms.max(500).min(5000);
    let start_request = format!(
        "{{\"method\":\"start\",\"arguments\":{{\"command\":\"echo deepseek-windows-tcp-smoke\",\"tty\":true,\"tty_rows\":24,\"tty_cols\":80,\"timeout_ms\":{wait_timeout}}}}}\n"
    );
    let start_response = shell_supervisor_tcp_request_raw(endpoint, "start", &start_request)?;
    let task_id = shell_supervisor_response_string(&start_response, "task_id")
        .ok_or_else(|| app_error("shell supervisor Windows TCP start response missing task_id"))?;
    if !start_response.contains(r#""job_pty_backend":"native-supervisor""#) {
        return Err(app_error(
            "shell supervisor Windows TCP smoke did not start a native-supervisor ConPTY job",
        ));
    }
    let cursor_response_request = format!(
        "{{\"method\":\"stdin\",\"arguments\":{{\"task_id\":\"{}\",\"input_base64\":\"G1sxOzFS\",\"timeout_ms\":{wait_timeout}}}}}\n",
        json_escape(&task_id)
    );
    let _ = shell_supervisor_tcp_request_raw(endpoint, "stdin", &cursor_response_request);

    let wait_request = format!(
        "{{\"method\":\"wait\",\"arguments\":{{\"task_id\":\"{}\",\"timeout_ms\":{wait_timeout}}}}}\n",
        json_escape(&task_id)
    );
    shell_supervisor_tcp_request_raw(endpoint, "wait", &wait_request)?;

    let attach_request = format!(
        "{{\"method\":\"attach\",\"arguments\":{{\"task_id\":\"{}\",\"tail\":true,\"limit_bytes\":4096}}}}\n",
        json_escape(&task_id)
    );
    let attach_response = shell_supervisor_tcp_request_raw(endpoint, "attach", &attach_request)?;
    if !attach_response.contains("deepseek-windows-tcp-smoke") {
        return Err(app_error(
            "shell supervisor Windows TCP attach smoke did not replay command output",
        ));
    }

    let stream_request = format!(
        "{{\"method\":\"attach_stream\",\"arguments\":{{\"task_id\":\"{}\",\"cursor\":0,\"limit_bytes\":4096,\"max_ms\":500,\"max_events\":1,\"poll_ms\":25}}}}\n",
        json_escape(&task_id)
    );
    let stream_response =
        shell_supervisor_tcp_request_raw(endpoint, "attach_stream", &stream_request)?;
    if !stream_response.contains("deepseek-windows-tcp-smoke")
        || !stream_response.contains(r#""stream_method":"attach""#)
    {
        return Err(app_error(
            "shell supervisor Windows TCP attach_stream smoke did not replay command output",
        ));
    }

    let resize_start_request = format!(
        "{{\"method\":\"start\",\"arguments\":{{\"command\":\"ping -n 6 127.0.0.1\",\"tty\":true,\"tty_rows\":24,\"tty_cols\":80,\"timeout_ms\":{wait_timeout}}}}}\n"
    );
    let resize_start_response =
        shell_supervisor_tcp_request_raw(endpoint, "start", &resize_start_request)?;
    let resize_task_id = shell_supervisor_response_string(&resize_start_response, "task_id")
        .ok_or_else(|| app_error("shell supervisor Windows TCP resize response missing task_id"))?;
    if !resize_start_response.contains(r#""job_pty_backend":"native-supervisor""#) {
        return Err(app_error(
            "shell supervisor Windows TCP resize smoke did not start a native-supervisor ConPTY job",
        ));
    }
    let resize_request = format!(
        "{{\"method\":\"resize\",\"arguments\":{{\"task_id\":\"{}\",\"tty_rows\":31,\"tty_cols\":99}}}}\n",
        json_escape(&resize_task_id)
    );
    let resize_response = shell_supervisor_tcp_request_raw(endpoint, "resize", &resize_request)?;
    if !resize_response.contains("meta.live_resize=windows_conpty") {
        return Err(app_error(format!(
            "shell supervisor Windows TCP resize smoke did not use windows_conpty: {}",
            resize_response.trim()
        )));
    }
    let cancel_request = format!(
        "{{\"method\":\"cancel\",\"arguments\":{{\"task_id\":\"{}\"}}}}\n",
        json_escape(&resize_task_id)
    );
    shell_supervisor_tcp_request_raw(endpoint, "cancel", &cancel_request)?;

    Ok(format!(
        "Windows TCP shell supervisor start/wait/attach/attach_stream/resize smoke passed for tasks {task_id}/{resize_task_id}"
    ))
}

#[cfg(unix)]
fn shell_supervisor_pty_control_smoke(socket: &Path, timeout_ms: u64) -> AppResult<String> {
    let command = "cat -";
    let start_request = format!(
        "{{\"method\":\"start\",\"arguments\":{{\"command\":\"{}\",\"tty\":true,\"tty_rows\":24,\"tty_cols\":80,\"timeout_ms\":{}}}}}\n",
        json_escape(command),
        timeout_ms
    );
    let start_response = shell_supervisor_request_raw(socket, "start", &start_request)?;
    let task_id = shell_supervisor_response_string(&start_response, "task_id")
        .ok_or_else(|| app_error("shell supervisor PTY smoke response missing task_id"))?;
    if !start_response.contains(r#""job_pty_backend":"native-supervisor""#) {
        return Err(app_error(
            "shell supervisor PTY smoke did not start a native-supervisor PTY job",
        ));
    }

    let smoke = (|| {
        let stdin_request = format!(
            "{{\"method\":\"stdin\",\"arguments\":{{\"task_id\":\"{}\",\"input\":\"deepseek-pty-control\\n\",\"timeout_ms\":500}}}}\n",
            json_escape(&task_id)
        );
        shell_supervisor_request_raw(socket, "stdin", &stdin_request)?;

        let resize_request = format!(
            "{{\"method\":\"resize\",\"arguments\":{{\"task_id\":\"{}\",\"tty_rows\":31,\"tty_cols\":99}}}}\n",
            json_escape(&task_id)
        );
        let resize_response = shell_supervisor_request_raw(socket, "resize", &resize_request)?;
        if !resize_response.contains("meta.live_resize=native_tiocswinsz") {
            return Err(app_error(
                "shell supervisor PTY resize smoke did not use native_tiocswinsz",
            ));
        }

        let attach_response = shell_supervisor_poll_for_output(
            socket,
            "attach",
            &task_id,
            "deepseek-pty-control",
            timeout_ms,
        )?;
        if !attach_response.contains("deepseek-pty-control") {
            return Err(app_error(
                "shell supervisor PTY attach smoke did not replay stdin output",
            ));
        }

        let replay_response = shell_supervisor_poll_for_output(
            socket,
            "replay",
            &task_id,
            "deepseek-pty-control",
            timeout_ms,
        )?;
        if !replay_response.contains("rows=31 cols=99") {
            return Err(app_error(
                "shell supervisor PTY replay smoke did not include resize event",
            ));
        }

        Ok(format!(
            "PTY stdin/resize/replay smoke passed for task {task_id}"
        ))
    })();

    let cancel_request = format!(
        "{{\"method\":\"cancel\",\"arguments\":{{\"task_id\":\"{}\"}}}}\n",
        json_escape(&task_id)
    );
    let cancel = shell_supervisor_request_raw(socket, "cancel", &cancel_request);
    match (smoke, cancel) {
        (Ok(summary), Ok(cancel_response)) => {
            if !cancel_response.contains("Canceled")
                && !cancel_response.contains("status: killed")
                && !cancel_response.contains("status: completed")
            {
                return Err(app_error(format!(
                    "shell supervisor PTY cancel smoke returned unexpected response: {}",
                    cancel_response.trim()
                )));
            }
            Ok(format!("{summary}; PTY cancel smoke passed"))
        }
        (Err(error), Ok(_)) => Err(error),
        (Ok(_), Err(cancel_error)) => Err(app_error(format!(
            "shell supervisor PTY cancel smoke failed: {cancel_error}"
        ))),
        (Err(error), Err(cancel_error)) => Err(app_error(format!(
            "{error}; cleanup cancel also failed: {cancel_error}"
        ))),
    }
}

#[cfg(unix)]
fn shell_supervisor_byte_stream_proxy_smoke(
    socket: &Path,
    timeout_ms: u64,
    cli_smoke: Option<(&str, &Path)>,
) -> AppResult<String> {
    let wait_timeout = timeout_ms.max(500).min(5000);
    let duplex_task_id = shell_supervisor_start_native_pty_smoke(
        socket,
        "head -n 1",
        wait_timeout,
        "byte_stream duplex",
    )?;
    shell_supervisor_byte_stream_duplex_control_smoke(socket, &duplex_task_id, wait_timeout)?;
    let raw_task_id = shell_supervisor_start_native_pty_smoke(
        socket,
        "head -n 1",
        wait_timeout,
        "byte_stream raw_proxy",
    )?;
    shell_supervisor_byte_stream_raw_proxy_control_smoke(socket, &raw_task_id, wait_timeout)?;
    let mut task_ids = vec![duplex_task_id, raw_task_id];
    let mut modes = "duplex/raw_proxy".to_string();
    if let Some((binary, workdir)) = cli_smoke {
        let fd_task_id = shell_supervisor_start_native_pty_smoke(
            socket,
            r#"echo fd-ready; IFS= read -r line; stty size; printf 'fd:%s\n' "$line""#,
            wait_timeout,
            "pty_fd handoff",
        )?;
        shell_supervisor_pty_fd_handoff_smoke(socket, &fd_task_id, wait_timeout)?;
        task_ids.push(fd_task_id);
        modes.push_str("/fd_handoff");
        let human_task_id = shell_supervisor_start_native_pty_smoke(
            socket,
            r#"echo proxy-ready; IFS= read -r line; stty size; printf 'proxy:%s\n' "$line""#,
            wait_timeout,
            "human proxy",
        )?;
        shell_supervisor_human_proxy_cli_smoke(
            socket,
            binary,
            workdir,
            &human_task_id,
            wait_timeout,
        )?;
        task_ids.push(human_task_id);
        modes.push_str("/human_proxy");
    }

    Ok(format!(
        "byte_stream {modes} smoke passed for tasks {}",
        task_ids.join("/")
    ))
}

#[cfg(all(unix, target_os = "linux"))]
fn shell_supervisor_pty_fd_handoff_smoke(
    socket: &Path,
    task_id: &str,
    timeout_ms: u64,
) -> AppResult<()> {
    use std::os::unix::net::UnixStream;

    let mut stream = UnixStream::connect(socket)?;
    let timeout = Duration::from_millis(timeout_ms.max(500).min(5000));
    stream.set_read_timeout(Some(timeout))?;
    stream.set_write_timeout(Some(timeout))?;
    let request = format!(
        "{{\"method\":\"pty_fd\",\"arguments\":{{\"task_id\":\"{}\",\"tty_rows\":37,\"tty_cols\":105,\"max_ms\":{}}}}}\n",
        json_escape(task_id),
        timeout_ms.max(500).min(5000)
    );
    stream.write_all(request.as_bytes())?;
    stream.flush()?;
    let response_line = read_shell_supervisor_request_line(&mut stream)?;
    if !response_line.contains(r#""status":"ok""#)
        || !response_line.contains(r#""stream_method":"pty_fd_handoff""#)
        || !response_line.contains(r#""handoff":"scm_rights""#)
    {
        return Err(app_error(format!(
            "shell supervisor pty_fd smoke returned unexpected handshake: {}",
            response_line.trim()
        )));
    }
    let mut pty = receive_shell_supervisor_fd(&stream)?;
    shell_fd_proxy_set_nonblocking(&pty)?;
    pty.write_all(b"fd-probe\n")?;
    pty.flush()?;
    let output = read_shell_fd_proxy_output_for(&mut pty, timeout);
    let output = String::from_utf8_lossy(&output);
    let _ = stream.shutdown(Shutdown::Both);
    if !output.contains("fd:fd-probe") || !output.contains("37 105") {
        return Err(app_error(format!(
            "shell supervisor pty_fd smoke missed direct fd PTY output: {}",
            output.trim()
        )));
    }
    Ok(())
}

#[cfg(all(unix, not(target_os = "linux")))]
fn shell_supervisor_pty_fd_handoff_smoke(
    _socket: &Path,
    _task_id: &str,
    _timeout_ms: u64,
) -> AppResult<()> {
    Err(app_error(
        "shell supervisor pty_fd handoff smoke requires Linux SCM_RIGHTS",
    ))
}

#[cfg(unix)]
fn shell_supervisor_start_native_pty_smoke(
    socket: &Path,
    command: &str,
    timeout_ms: u64,
    label: &str,
) -> AppResult<String> {
    let start_request = format!(
        "{{\"method\":\"start\",\"arguments\":{{\"command\":\"{}\",\"tty\":true,\"tty_rows\":24,\"tty_cols\":80,\"timeout_ms\":{}}}}}\n",
        json_escape(command),
        timeout_ms
    );
    let start_response = shell_supervisor_request_raw(socket, "start", &start_request)?;
    let task_id =
        shell_supervisor_response_string(&start_response, "task_id").ok_or_else(|| {
            app_error(format!(
                "shell supervisor {label} smoke response missing task_id"
            ))
        })?;
    if !start_response.contains(r#""job_pty_backend":"native-supervisor""#) {
        return Err(app_error(format!(
            "shell supervisor {label} smoke did not start a native-supervisor PTY job"
        )));
    }
    Ok(task_id)
}

#[cfg(unix)]
fn shell_supervisor_byte_stream_duplex_control_smoke(
    socket: &Path,
    task_id: &str,
    timeout_ms: u64,
) -> AppResult<()> {
    use std::os::unix::net::UnixStream;

    let mut stream = UnixStream::connect(socket)?;
    let timeout = Duration::from_millis(timeout_ms.max(500).min(5000));
    stream.set_read_timeout(Some(timeout))?;
    stream.set_write_timeout(Some(timeout))?;
    let request = format!(
        "{{\"method\":\"byte_stream\",\"arguments\":{{\"task_id\":\"{}\",\"cursor\":0,\"limit_bytes\":4096,\"max_ms\":{},\"max_events\":12,\"poll_ms\":25}}}}\n",
        json_escape(task_id),
        timeout_ms.max(500).min(5000)
    );
    stream.write_all(request.as_bytes())?;
    stream.write_all(br#"{"type":"resize","rows":36,"cols":104}"#)?;
    stream.write_all(b"\n")?;
    stream.write_all(br#"{"type":"stdin","input":"frame-probe\n"}"#)?;
    stream.write_all(b"\n")?;
    stream.shutdown(std::net::Shutdown::Write)?;

    let mut reader = BufReader::new(stream);
    let mut body = String::new();
    loop {
        let mut line = String::new();
        let read = reader.read_line(&mut line)?;
        if read == 0 {
            break;
        }
        body.push_str(&line);
        if line.contains(r#""stream_done":true"#) {
            break;
        }
    }
    if !body.contains(r#""method":"byte_stream""#)
        || !body.contains(r#""stream_method":"pty_byte_stream""#)
    {
        return Err(app_error(format!(
            "shell supervisor byte_stream duplex smoke returned unexpected body: {}",
            body.trim()
        )));
    }
    if !body.contains(r#""control_frames""#)
        || !body.contains(r#""byte_outputs""#)
        || !body.contains(r#""bytes_base64""#)
    {
        return Err(app_error(format!(
            "shell supervisor byte_stream duplex smoke missed control/raw byte evidence: {}",
            body.trim()
        )));
    }
    if !body.contains("meta.live_resize=native_tiocswinsz") || !body.contains("frame-probe") {
        return Err(app_error(format!(
            "shell supervisor byte_stream duplex smoke missed resize/stdin output: {}",
            body.trim()
        )));
    }
    Ok(())
}

#[cfg(unix)]
fn shell_supervisor_byte_stream_raw_proxy_control_smoke(
    socket: &Path,
    task_id: &str,
    timeout_ms: u64,
) -> AppResult<()> {
    use std::os::unix::net::UnixStream;

    let mut stream = UnixStream::connect(socket)?;
    let timeout = Duration::from_millis(timeout_ms.max(500).min(5000));
    stream.set_read_timeout(Some(timeout))?;
    stream.set_write_timeout(Some(timeout))?;
    let request = format!(
        "{{\"method\":\"byte_stream\",\"arguments\":{{\"task_id\":\"{}\",\"cursor\":0,\"limit_bytes\":4096,\"max_ms\":{},\"poll_ms\":10,\"raw_proxy\":true}}}}\n",
        json_escape(task_id),
        timeout_ms.max(500).min(5000)
    );
    stream.write_all(request.as_bytes())?;
    stream.write_all(b"raw-probe\n")?;
    stream.shutdown(std::net::Shutdown::Write)?;

    let mut body = Vec::new();
    stream.read_to_end(&mut body)?;
    let body = String::from_utf8_lossy(&body);
    if body.contains("byte_outputs") || body.contains("\"method\":\"byte_stream\"") {
        return Err(app_error(format!(
            "shell supervisor byte_stream raw_proxy smoke returned JSON instead of raw bytes: {}",
            body.trim()
        )));
    }
    if !body.contains("raw-probe") {
        return Err(app_error(format!(
            "shell supervisor byte_stream raw_proxy smoke missed raw PTY output: {}",
            body.trim()
        )));
    }
    Ok(())
}

#[cfg(all(unix, target_os = "linux"))]
fn shell_supervisor_human_proxy_cli_smoke(
    socket: &Path,
    binary: &str,
    workdir: &Path,
    task_id: &str,
    timeout_ms: u64,
) -> AppResult<()> {
    use std::os::unix::process::CommandExt;

    let (mut master, slave) = open_shell_fixture_proxy_pty(35, 103)?;
    let stdin = slave.try_clone()?;
    let stdout = slave.try_clone()?;
    let stderr = slave;
    let mut command = Command::new(binary);
    unsafe {
        command.pre_exec(|| {
            if shell_fixture_proxy_setsid() < 0 {
                return Err(std::io::Error::last_os_error());
            }
            if shell_fixture_proxy_ioctl(0, SHELL_FIXTURE_PROXY_TIOCSCTTY, 0) < 0 {
                return Err(std::io::Error::last_os_error());
            }
            if shell_fixture_proxy_tcsetpgrp(0, shell_fixture_proxy_getpid()) < 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let max_ms = timeout_ms.max(500).min(5000).to_string();
    let mut child = command
        .args([
            "agents",
            "shell",
            "proxy",
            task_id,
            "--max-ms",
            &max_ms,
            "--poll-ms",
            "10",
            "--limit-bytes",
            "4096",
        ])
        .current_dir(workdir)
        .env("TERM", "xterm-256color")
        .stdin(Stdio::from(stdin))
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr))
        .spawn()
        .map_err(|error| app_error(format!("failed to spawn agents shell proxy smoke: {error}")))?;
    std::thread::sleep(Duration::from_millis(100));
    master.write_all(b"fixture-proxy\n")?;
    master.flush()?;

    let deadline = Instant::now() + Duration::from_millis(timeout_ms.max(1000).min(7000));
    let mut timed_out = false;
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if Instant::now() >= deadline {
            timed_out = true;
            terminate_child(&mut child);
            break child.wait()?;
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    let transcript = read_shell_fixture_proxy_pty_output(&mut master);
    let transcript = String::from_utf8_lossy(&transcript);
    if timed_out || !status.success() {
        return Err(app_error(format!(
            "agents shell proxy smoke failed with {status}; timed_out={timed_out}; transcript: {}",
            transcript.trim()
        )));
    }
    if !transcript.contains("proxied to") || !transcript.contains("fixture-proxy") {
        return Err(app_error(format!(
            "agents shell proxy smoke did not show proxy transcript: {}",
            transcript.trim()
        )));
    }

    let replay_response = shell_supervisor_poll_for_output(
        socket,
        "replay",
        task_id,
        "proxy:fixture-proxy",
        timeout_ms,
    )?;
    if !replay_response.contains("35 103") {
        return Err(app_error(format!(
            "agents shell proxy smoke did not sync terminal size to child PTY: {}",
            replay_response.trim()
        )));
    }
    Ok(())
}

#[cfg(all(unix, not(target_os = "linux")))]
fn shell_supervisor_human_proxy_cli_smoke(
    _socket: &Path,
    _binary: &str,
    _workdir: &Path,
    _task_id: &str,
    _timeout_ms: u64,
) -> AppResult<()> {
    Err(app_error(
        "agents shell proxy fixture smoke currently requires Linux PTY helpers",
    ))
}

#[cfg(all(unix, target_os = "linux"))]
fn send_shell_supervisor_fd(
    stream: &std::os::unix::net::UnixStream,
    fd: std::os::fd::RawFd,
) -> AppResult<()> {
    use std::os::fd::AsRawFd;

    let mut byte = [b'F'];
    let mut iov = ShellFdIovec {
        iov_base: byte.as_mut_ptr().cast(),
        iov_len: byte.len(),
    };
    let fd_bytes = fd.to_ne_bytes();
    let control_len = shell_fd_cmsg_space(fd_bytes.len());
    let mut control = vec![0u8; control_len];
    let header = control.as_mut_ptr().cast::<ShellFdCmsghdr>();
    unsafe {
        (*header).cmsg_len = shell_fd_cmsg_len(fd_bytes.len());
        (*header).cmsg_level = SHELL_FD_SOL_SOCKET;
        (*header).cmsg_type = SHELL_FD_SCM_RIGHTS;
        std::ptr::copy_nonoverlapping(
            fd_bytes.as_ptr(),
            control
                .as_mut_ptr()
                .add(shell_fd_cmsg_align(std::mem::size_of::<ShellFdCmsghdr>())),
            fd_bytes.len(),
        );
    }
    let msg = ShellFdMsghdr {
        msg_name: std::ptr::null_mut(),
        msg_namelen: 0,
        msg_iov: &mut iov,
        msg_iovlen: 1,
        msg_control: control.as_mut_ptr().cast(),
        msg_controllen: control.len(),
        msg_flags: 0,
    };
    let sent = unsafe { shell_fd_sendmsg(stream.as_raw_fd(), &msg, 0) };
    if sent < 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok(())
}

#[cfg(all(unix, target_os = "linux"))]
fn receive_shell_supervisor_fd(
    stream: &std::os::unix::net::UnixStream,
) -> AppResult<std::fs::File> {
    use std::os::fd::{AsRawFd, FromRawFd};

    let mut byte = [0u8; 1];
    let mut iov = ShellFdIovec {
        iov_base: byte.as_mut_ptr().cast(),
        iov_len: byte.len(),
    };
    let mut control = vec![0u8; shell_fd_cmsg_space(std::mem::size_of::<std::os::fd::RawFd>())];
    let mut msg = ShellFdMsghdr {
        msg_name: std::ptr::null_mut(),
        msg_namelen: 0,
        msg_iov: &mut iov,
        msg_iovlen: 1,
        msg_control: control.as_mut_ptr().cast(),
        msg_controllen: control.len(),
        msg_flags: 0,
    };
    let received = unsafe { shell_fd_recvmsg(stream.as_raw_fd(), &mut msg, 0) };
    if received < 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    if received == 0 {
        return Err(app_error(
            "shell supervisor pty_fd did not send a file descriptor",
        ));
    }
    let header = control.as_ptr().cast::<ShellFdCmsghdr>();
    let (level, kind, len) = unsafe {
        (
            (*header).cmsg_level,
            (*header).cmsg_type,
            (*header).cmsg_len,
        )
    };
    if level != SHELL_FD_SOL_SOCKET
        || kind != SHELL_FD_SCM_RIGHTS
        || len < shell_fd_cmsg_len(std::mem::size_of::<std::os::fd::RawFd>())
    {
        return Err(app_error(
            "shell supervisor pty_fd response did not include SCM_RIGHTS",
        ));
    }
    let mut fd_bytes = [0u8; std::mem::size_of::<std::os::fd::RawFd>()];
    unsafe {
        std::ptr::copy_nonoverlapping(
            control
                .as_ptr()
                .add(shell_fd_cmsg_align(std::mem::size_of::<ShellFdCmsghdr>())),
            fd_bytes.as_mut_ptr(),
            fd_bytes.len(),
        );
    }
    let fd = std::os::fd::RawFd::from_ne_bytes(fd_bytes);
    if fd < 0 {
        return Err(app_error("shell supervisor pty_fd returned an invalid fd"));
    }
    Ok(unsafe { std::fs::File::from_raw_fd(fd) })
}

#[cfg(all(unix, target_os = "linux"))]
fn shell_fd_cmsg_align(len: usize) -> usize {
    let align = std::mem::size_of::<usize>();
    (len + align - 1) & !(align - 1)
}

#[cfg(all(unix, target_os = "linux"))]
fn shell_fd_cmsg_len(data_len: usize) -> usize {
    shell_fd_cmsg_align(std::mem::size_of::<ShellFdCmsghdr>()) + data_len
}

#[cfg(all(unix, target_os = "linux"))]
fn shell_fd_cmsg_space(data_len: usize) -> usize {
    shell_fd_cmsg_align(std::mem::size_of::<ShellFdCmsghdr>()) + shell_fd_cmsg_align(data_len)
}

#[cfg(all(unix, target_os = "linux"))]
fn shell_fd_proxy_set_nonblocking(file: &std::fs::File) -> AppResult<()> {
    use std::os::fd::AsRawFd;

    let fd = file.as_raw_fd();
    let flags = unsafe { shell_fixture_proxy_fcntl(fd, SHELL_FD_F_GETFL) };
    if flags < 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    let result =
        unsafe { shell_fixture_proxy_fcntl(fd, SHELL_FD_F_SETFL, flags | SHELL_FD_O_NONBLOCK) };
    if result < 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok(())
}

#[cfg(all(unix, target_os = "linux"))]
fn shell_fd_proxy_set_winsize(fd: i32, rows: u16, cols: u16) -> AppResult<()> {
    let size = ShellFixtureProxyWinsize {
        ws_row: rows,
        ws_col: cols,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    let result = unsafe { shell_fixture_proxy_ioctl(fd, SHELL_FIXTURE_PROXY_TIOCSWINSZ, &size) };
    if result < 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok(())
}

#[cfg(all(unix, target_os = "linux"))]
#[repr(C)]
struct ShellFdIovec {
    iov_base: *mut std::ffi::c_void,
    iov_len: usize,
}

#[cfg(all(unix, target_os = "linux"))]
#[repr(C)]
struct ShellFdMsghdr {
    msg_name: *mut std::ffi::c_void,
    msg_namelen: u32,
    msg_iov: *mut ShellFdIovec,
    msg_iovlen: usize,
    msg_control: *mut std::ffi::c_void,
    msg_controllen: usize,
    msg_flags: i32,
}

#[cfg(all(unix, target_os = "linux"))]
#[repr(C)]
struct ShellFdCmsghdr {
    cmsg_len: usize,
    cmsg_level: i32,
    cmsg_type: i32,
}

#[cfg(all(unix, target_os = "linux"))]
const SHELL_FD_SOL_SOCKET: i32 = 1;
#[cfg(all(unix, target_os = "linux"))]
const SHELL_FD_SCM_RIGHTS: i32 = 1;
#[cfg(all(unix, target_os = "linux"))]
const SHELL_FD_F_GETFL: i32 = 3;
#[cfg(all(unix, target_os = "linux"))]
const SHELL_FD_F_SETFL: i32 = 4;
#[cfg(all(unix, target_os = "linux"))]
const SHELL_FD_O_NONBLOCK: i32 = 0o4000;

#[cfg(all(unix, target_os = "linux"))]
unsafe extern "C" {
    #[link_name = "sendmsg"]
    fn shell_fd_sendmsg(fd: i32, msg: *const ShellFdMsghdr, flags: i32) -> isize;
    #[link_name = "recvmsg"]
    fn shell_fd_recvmsg(fd: i32, msg: *mut ShellFdMsghdr, flags: i32) -> isize;
}

#[cfg(all(unix, target_os = "linux"))]
#[repr(C)]
struct ShellFixtureProxyWinsize {
    ws_row: u16,
    ws_col: u16,
    ws_xpixel: u16,
    ws_ypixel: u16,
}

#[cfg(all(unix, target_os = "linux"))]
unsafe extern "C" {
    fn posix_openpt(flags: i32) -> i32;
    fn grantpt(fd: i32) -> i32;
    fn unlockpt(fd: i32) -> i32;
    fn ptsname(fd: i32) -> *mut std::os::raw::c_char;
    #[link_name = "setsid"]
    fn shell_fixture_proxy_setsid() -> i32;
    #[link_name = "getpid"]
    fn shell_fixture_proxy_getpid() -> i32;
    #[link_name = "tcsetpgrp"]
    fn shell_fixture_proxy_tcsetpgrp(fd: i32, pgrp: i32) -> i32;
    #[link_name = "ioctl"]
    fn shell_fixture_proxy_ioctl(fd: i32, request: u64, ...) -> i32;
    #[link_name = "fcntl"]
    fn shell_fixture_proxy_fcntl(fd: i32, cmd: i32, ...) -> i32;
}

#[cfg(all(unix, target_os = "linux"))]
const SHELL_FIXTURE_PROXY_TIOCSCTTY: u64 = 0x540E;
#[cfg(all(unix, target_os = "linux"))]
const SHELL_FIXTURE_PROXY_TIOCSWINSZ: u64 = 0x5414;

#[cfg(all(unix, target_os = "linux"))]
fn open_shell_fixture_proxy_pty(rows: u16, cols: u16) -> AppResult<(std::fs::File, std::fs::File)> {
    use std::ffi::CStr;
    use std::os::fd::{AsRawFd, FromRawFd};

    const O_RDWR: i32 = 0x0002;
    const O_NOCTTY: i32 = 0x0100;

    let master_fd = unsafe { posix_openpt(O_RDWR | O_NOCTTY) };
    if master_fd < 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    if unsafe { grantpt(master_fd) } < 0 {
        let error = std::io::Error::last_os_error();
        unsafe {
            let _ = std::fs::File::from_raw_fd(master_fd);
        }
        return Err(error.into());
    }
    if unsafe { unlockpt(master_fd) } < 0 {
        let error = std::io::Error::last_os_error();
        unsafe {
            let _ = std::fs::File::from_raw_fd(master_fd);
        }
        return Err(error.into());
    }
    let slave_name = unsafe { ptsname(master_fd) };
    if slave_name.is_null() {
        let error = std::io::Error::last_os_error();
        unsafe {
            let _ = std::fs::File::from_raw_fd(master_fd);
        }
        return Err(error.into());
    }
    let slave_path = unsafe { CStr::from_ptr(slave_name) }
        .to_string_lossy()
        .to_string();
    let master = unsafe { std::fs::File::from_raw_fd(master_fd) };
    let slave = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(slave_path)?;
    let size = ShellFixtureProxyWinsize {
        ws_row: rows,
        ws_col: cols,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    let result = unsafe {
        shell_fixture_proxy_ioctl(slave.as_raw_fd(), SHELL_FIXTURE_PROXY_TIOCSWINSZ, &size)
    };
    if result < 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok((master, slave))
}

#[cfg(all(unix, target_os = "linux"))]
fn read_shell_fixture_proxy_pty_output(master: &mut std::fs::File) -> Vec<u8> {
    use std::os::fd::AsRawFd;

    const F_GETFL: i32 = 3;
    const F_SETFL: i32 = 4;
    const O_NONBLOCK: i32 = 0x0800;

    let flags = unsafe { shell_fixture_proxy_fcntl(master.as_raw_fd(), F_GETFL, 0) };
    if flags >= 0 {
        let _ =
            unsafe { shell_fixture_proxy_fcntl(master.as_raw_fd(), F_SETFL, flags | O_NONBLOCK) };
    }
    let mut output = Vec::new();
    let mut buffer = [0u8; 4096];
    loop {
        match master.read(&mut buffer) {
            Ok(0) => break,
            Ok(count) => output.extend_from_slice(&buffer[..count]),
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
            Err(_) => break,
        }
    }
    output
}

#[cfg(unix)]
fn shell_supervisor_poll_for_output(
    socket: &Path,
    method: &str,
    task_id: &str,
    needle: &str,
    timeout_ms: u64,
) -> AppResult<String> {
    let deadline = Instant::now() + Duration::from_millis(timeout_ms.max(100).min(5000));
    let mut last_response = String::new();
    loop {
        if Instant::now() >= deadline {
            return Err(app_error(format!(
                "timed out waiting for shell supervisor {method} output `{needle}`; last response: {}",
                last_response.trim()
            )));
        }
        let request = match method {
            "attach" => format!(
                "{{\"method\":\"attach\",\"arguments\":{{\"task_id\":\"{}\",\"tail\":true,\"limit_bytes\":4096}}}}\n",
                json_escape(task_id)
            ),
            "replay" => format!(
                "{{\"method\":\"replay\",\"arguments\":{{\"task_id\":\"{}\",\"stream\":\"terminal\",\"cursor\":0,\"limit_bytes\":4096}}}}\n",
                json_escape(task_id)
            ),
            _ => return Err(app_error(format!("unsupported shell supervisor poll method: {method}"))),
        };
        let response = shell_supervisor_request_raw(socket, method, &request)?;
        if response.contains(needle) {
            return Ok(response);
        }
        last_response = response;
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn wait_child_exit(child: &mut Child, timeout: Duration) -> AppResult<std::process::ExitStatus> {
    let start = Instant::now();
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(status);
        }
        if start.elapsed() >= timeout {
            return Err(app_error("timed out waiting for child process exit"));
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn terminate_child(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

fn read_child_stderr(child: &mut Child) -> String {
    let mut output = String::new();
    if let Some(stderr) = child.stderr.as_mut() {
        let _ = stderr.read_to_string(&mut output);
    }
    output.trim().to_string()
}

fn child_stderr_suffix(stderr: &str) -> String {
    if stderr.is_empty() {
        String::new()
    } else {
        format!("; stderr: {stderr}")
    }
}

fn service_check_blocker_count(checks: &[ServiceDoctorCheck]) -> usize {
    checks
        .iter()
        .filter(|check| check.status == ServiceDoctorStatus::Blocker)
        .count()
}

fn service_check_warning_count(checks: &[ServiceDoctorCheck]) -> usize {
    checks
        .iter()
        .filter(|check| check.status == ServiceDoctorStatus::Warn)
        .count()
}

fn render_service_smoke_text(report: &ServiceSmokeReport) -> String {
    let mut out = String::new();
    out.push_str("DeepSeekCode service smoke\n");
    out.push_str(&format!("  kind: {}\n", service_kind_label(report.kind)));
    out.push_str(&format!("  installed: {}\n", report.installed));
    out.push_str(&format!("  binary: {}\n", report.binary));
    out.push_str(&format!("  workdir: {}\n", report.workdir.display()));
    out.push_str(&format!("  requested_addr: {}\n", report.requested_addr));
    out.push_str(&format!("  resolved_addr: {}\n", report.resolved_addr));
    out.push_str(&format!("  timeout_ms: {}\n\n", report.timeout_ms));
    for check in &report.checks {
        out.push_str(&format!(
            "[{}] {}: {}\n",
            service_doctor_status_label(check.status),
            check.name,
            check.message
        ));
    }
    out.push_str(&format!(
        "\nsummary: {} blocker(s), {} warning(s)\n",
        service_check_blocker_count(&report.checks),
        service_check_warning_count(&report.checks)
    ));
    out
}

fn render_service_smoke_json(report: &ServiceSmokeReport) -> String {
    let checks = report
        .checks
        .iter()
        .map(|check| {
            format!(
                "{{\"status\":\"{}\",\"name\":\"{}\",\"message\":\"{}\"}}",
                service_doctor_status_label(check.status),
                json_escape(&check.name),
                json_escape(&check.message)
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "{{\"kind\":\"deepseek.agents.service_smoke.v1\",\"service_kind\":\"{}\",\"installed\":{},\"binary\":\"{}\",\"workdir\":\"{}\",\"requested_addr\":\"{}\",\"resolved_addr\":\"{}\",\"timeout_ms\":{},\"blockers\":{},\"warnings\":{},\"checks\":[{}]}}",
        service_kind_label(report.kind),
        report.installed,
        json_escape(&report.binary),
        json_escape(&report.workdir.display().to_string()),
        json_escape(&report.requested_addr),
        json_escape(&report.resolved_addr),
        report.timeout_ms,
        service_check_blocker_count(&report.checks),
        service_check_warning_count(&report.checks),
        checks
    )
}

fn run_runtime_daemon_tick(
    config: &AppConfig,
    store: &RuntimeStore,
    budget: Option<usize>,
    json: bool,
) -> AppResult<RuntimeDaemonTick> {
    let mut tick = RuntimeDaemonTick::default();
    let now = current_epoch_seconds();

    for automation in store.list_automations(None, None, 1_000)? {
        if !automation_is_due(&automation, now) {
            continue;
        }
        match store.trigger_automation(&automation.id, None) {
            Ok((updated, task)) => {
                tick.triggered_automations += 1;
                let next_run_at = next_run_for_schedule(&updated.schedule, now);
                let updated = store.update_automation_next_run(&updated.id, next_run_at)?;
                if json {
                    println!(
                        "{}",
                        json_value_to_string(&daemon_automation_event(&updated, &task))
                    );
                }
            }
            Err(error) => {
                tick.failed_automations += 1;
                if json {
                    println!(
                        "{}",
                        json_value_to_string(&daemon_error_event(
                            "automation_failed",
                            &automation.id,
                            &error.to_string(),
                        ))
                    );
                } else {
                    eprintln!("automation {} failed: {error}", automation.id);
                }
            }
        }
    }

    run_runtime_daemon_rlm_recovery(config, json, &mut tick)?;
    run_runtime_daemon_rlm_live_turn(config, store, json, &mut tick)?;

    let mut pending = store
        .list_tasks(None, None, 1_000)?
        .into_iter()
        .filter(|task| {
            task.status == "pending" && task.thread_id.is_some() && task.kind != "rlm_process"
        })
        .collect::<Vec<_>>();
    pending.sort_by(|a, b| {
        a.created_at
            .cmp(&b.created_at)
            .then_with(|| a.id.cmp(&b.id))
    });

    for task in pending.into_iter().take(1) {
        match run_runtime_task(config.clone(), &task.id, budget, json) {
            Ok(()) => tick.executed_tasks += 1,
            Err(error) => {
                tick.failed_tasks += 1;
                if json {
                    println!(
                        "{}",
                        json_value_to_string(&daemon_error_event(
                            "task_failed",
                            &task.id,
                            &error.to_string(),
                        ))
                    );
                } else {
                    eprintln!("task {} failed: {error}", task.id);
                }
            }
        }
    }

    run_runtime_daemon_compactions(config, store, json, &mut tick)?;

    Ok(tick)
}

fn run_runtime_daemon_rlm_recovery(
    config: &AppConfig,
    json: bool,
    tick: &mut RuntimeDaemonTick,
) -> AppResult<()> {
    let output = RlmLiveRecoverTool {
        config: config.clone(),
    }
    .execute(
        ToolInput::new()
            .with_arg("all", "true")
            .with_arg("reason", "runtime daemon stale live RLM owner recovery"),
    );
    match output {
        Ok(output) => {
            let recovered_count = parse_json_value(&output.summary)
                .ok()
                .and_then(|value| match value {
                    JsonValue::Object(root) => root.get("recovered_count").and_then(json_as_u64),
                    _ => None,
                })
                .unwrap_or(0) as usize;
            tick.recovered_rlm_turns += recovered_count;
            if json && recovered_count > 0 {
                println!(
                    "{}",
                    json_value_to_string(&daemon_rlm_recovery_event(
                        recovered_count,
                        Some(&output.summary),
                    ))
                );
            }
        }
        Err(error) => {
            tick.failed_rlm_recoveries += 1;
            if json {
                println!(
                    "{}",
                    json_value_to_string(&daemon_error_event(
                        "rlm_recovery_failed",
                        "all",
                        &error.to_string(),
                    ))
                );
            } else {
                eprintln!("live RLM recovery failed: {error}");
            }
        }
    }
    Ok(())
}

fn run_runtime_daemon_rlm_live_turn(
    config: &AppConfig,
    store: &RuntimeStore,
    json: bool,
    tick: &mut RuntimeDaemonTick,
) -> AppResult<()> {
    let sessions_by_thread = rlm_live_session_ids_by_runtime_thread(config)?;
    if sessions_by_thread.is_empty() {
        return Ok(());
    }
    let mut pending = store
        .list_tasks(None, None, 1_000)?
        .into_iter()
        .filter(|task| {
            task.kind == "rlm_process"
                && task.status == "pending"
                && task
                    .thread_id
                    .as_ref()
                    .is_some_and(|thread_id| sessions_by_thread.contains_key(thread_id))
        })
        .collect::<Vec<_>>();
    pending.sort_by(|a, b| {
        a.created_at
            .cmp(&b.created_at)
            .then_with(|| a.id.cmp(&b.id))
    });
    let Some(task) = pending.into_iter().next() else {
        return Ok(());
    };
    let Some(thread_id) = task.thread_id.as_deref() else {
        return Ok(());
    };
    let Some(session_id) = sessions_by_thread.get(thread_id).cloned() else {
        return Ok(());
    };

    let output = RlmLiveRunNextTool {
        config: config.clone(),
        parent_depth: 0,
    }
    .execute(
        ToolInput::new()
            .with_arg("session_id", session_id.clone())
            .with_arg("task_id", task.id.clone()),
    );
    match output {
        Ok(output) => {
            tick.executed_rlm_turns += 1;
            if json {
                println!(
                    "{}",
                    json_value_to_string(&daemon_rlm_turn_event(
                        "rlm_turn_completed",
                        &session_id,
                        &task.id,
                        Some(&output.summary),
                    ))
                );
            }
        }
        Err(error) => {
            tick.failed_rlm_turns += 1;
            if json {
                println!(
                    "{}",
                    json_value_to_string(&daemon_rlm_turn_event(
                        "rlm_turn_failed",
                        &session_id,
                        &task.id,
                        Some(&error.to_string()),
                    ))
                );
            } else {
                eprintln!("live RLM turn {} failed: {error}", task.id);
            }
        }
    }
    Ok(())
}

fn run_runtime_daemon_compactions(
    config: &AppConfig,
    store: &RuntimeStore,
    json: bool,
    tick: &mut RuntimeDaemonTick,
) -> AppResult<()> {
    let settings = runtime_daemon_compaction_settings(config);
    run_runtime_daemon_compactions_with_summary_provider(
        store,
        json,
        tick,
        settings,
        |store, thread| {
            automatic_model_compaction_summary(config, store, thread, settings.keep_tail_turns)
        },
    )
}

fn run_runtime_daemon_compactions_with_summary_provider<F>(
    store: &RuntimeStore,
    json: bool,
    tick: &mut RuntimeDaemonTick,
    settings: RuntimeDaemonCompactionSettings,
    mut summary_provider: F,
) -> AppResult<()>
where
    F: FnMut(&RuntimeStore, &ThreadRecord) -> AppResult<Option<String>>,
{
    if settings.threshold_tokens == 0 {
        return Ok(());
    }
    for thread in store.list_threads(1_000)? {
        if !thread_needs_compaction(store, &thread, settings)? {
            continue;
        }
        let model_summary = match summary_provider(store, &thread) {
            Ok(summary) => summary,
            Err(error) => {
                if json {
                    println!(
                        "{}",
                        json_value_to_string(&daemon_error_event(
                            "compaction_summary_failed",
                            &thread.id,
                            &error.to_string(),
                        ))
                    );
                } else {
                    eprintln!("compaction summary {} failed: {error}", thread.id);
                }
                None
            }
        };
        let result = if let Some(summary) = model_summary {
            store.compact_thread_with_summary_source(
                &thread.id,
                settings.keep_tail_turns,
                summary,
                "model",
            )
        } else {
            store.compact_thread(&thread.id, settings.keep_tail_turns, None)
        };
        match result {
            Ok(compaction) => {
                tick.compacted_threads += 1;
                if json {
                    println!(
                        "{}",
                        json_value_to_string(&daemon_compaction_event(&compaction))
                    );
                }
            }
            Err(error) => {
                tick.failed_compactions += 1;
                if json {
                    println!(
                        "{}",
                        json_value_to_string(&daemon_error_event(
                            "compaction_failed",
                            &thread.id,
                            &error.to_string(),
                        ))
                    );
                } else {
                    eprintln!("compaction {} failed: {error}", thread.id);
                }
            }
        }
    }
    Ok(())
}

fn automatic_model_compaction_summary(
    config: &AppConfig,
    store: &RuntimeStore,
    thread: &ThreadRecord,
    keep_tail_turns: usize,
) -> AppResult<Option<String>> {
    if !model_api_key_configured(&config.model.api_key_env) {
        return Ok(None);
    }
    let turns = store.list_turns(&thread.id)?;
    if turns.len() <= keep_tail_turns {
        return Ok(None);
    }
    let client = DeepSeekClient {
        config: config.model.clone(),
    };
    model_compaction_summary_with_client(&client, thread, &turns, keep_tail_turns).map(Some)
}

fn model_api_key_configured(api_key_env: &str) -> bool {
    std::env::var(api_key_env)
        .ok()
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false)
}

fn model_compaction_summary_with_client<C: ModelClient>(
    client: &C,
    thread: &ThreadRecord,
    turns: &[TurnRecord],
    keep_tail_turns: usize,
) -> AppResult<String> {
    if keep_tail_turns >= turns.len() {
        return Err(app_error(
            "model compaction requires at least one older turn to summarize",
        ));
    }
    let request = build_model_compaction_request(thread, turns, keep_tail_turns);
    let mut events = crate::ui::stream::NoopStreamEvents;
    let (response, _usage) = client.respond(request, &mut events)?;
    if !matches!(response.action, ModelAction::Finish) {
        return Err(app_error(
            "model compaction summary returned a tool call; expected final summary text",
        ));
    }
    let summary = response.message.trim();
    if summary.is_empty() {
        return Err(app_error("model compaction summary was empty"));
    }
    Ok(summary.to_string())
}

fn build_model_compaction_request(
    thread: &ThreadRecord,
    turns: &[TurnRecord],
    keep_tail_turns: usize,
) -> ModelRequest {
    ModelRequest {
        system_prompt: "Summarize older durable runtime context for automatic compaction. Return only the summary text. Preserve user intent, decisions, constraints, changed files, tool outcomes, unresolved tasks, and anything the next assistant turn must remember. Be concise and actionable.".to_string(),
        task: render_model_compaction_task(thread, turns, keep_tail_turns),
        image_inputs: Vec::new(),
        profile_name: "runtime-compaction".to_string(),
        profile_hints: vec![
            "No tools are available.".to_string(),
            "Write a durable context summary, not a user-facing answer.".to_string(),
        ],
        primary_file: None,
        suggested_test_command: None,
        available_tools: Vec::new(),
        observations: Vec::new(),
        todos: Vec::new(),
        planning_mode: false,
        recent_steps: Vec::new(),
    }
}

fn render_model_compaction_task(
    thread: &ThreadRecord,
    turns: &[TurnRecord],
    keep_tail_turns: usize,
) -> String {
    const MAX_SUMMARIZED_TURNS: usize = 32;
    const MAX_TURN_CHARS: usize = 700;

    let split_at = turns.len().saturating_sub(keep_tail_turns);
    let summarized_turns = &turns[..split_at];
    let kept_turns = &turns[split_at..];
    let omitted = summarized_turns.len().saturating_sub(MAX_SUMMARIZED_TURNS);
    let summarized_window = summarized_turns.iter().skip(omitted).collect::<Vec<_>>();

    let mut task = String::new();
    task.push_str("Create a compact durable summary for older turns in this runtime thread.\n");
    task.push_str("Thread title: ");
    task.push_str(&thread.title);
    task.push('\n');
    task.push_str("Thread id: ");
    task.push_str(&thread.id);
    task.push('\n');
    task.push_str("Older turns to summarize: ");
    task.push_str(&summarized_turns.len().to_string());
    task.push('\n');
    task.push_str("Tail turns preserved verbatim: ");
    task.push_str(&kept_turns.len().to_string());
    task.push_str("\n\n");
    if omitted > 0 {
        task.push_str("The oldest ");
        task.push_str(&omitted.to_string());
        task.push_str(" summarized turn(s) were omitted from this bounded summary prompt.\n\n");
    }
    task.push_str("Summarized turn window:\n");
    for turn in summarized_window {
        task.push_str("- #");
        task.push_str(&turn.index.to_string());
        task.push(' ');
        task.push_str(&turn.role);
        task.push_str(" turn_id=");
        task.push_str(&turn.id);
        task.push_str(": ");
        task.push_str(&compaction_excerpt(&turn.content, MAX_TURN_CHARS));
        task.push('\n');
    }
    if let Some(first_kept) = kept_turns.first() {
        task.push_str("\nThe live tail begins at turn #");
        task.push_str(&first_kept.index.to_string());
        task.push_str(" (turn_id=");
        task.push_str(&first_kept.id);
        task.push_str(
            "). Do not restate the tail verbatim; summarize only what older turns establish.\n",
        );
    }
    task
}

fn compaction_excerpt(content: &str, max_chars: usize) -> String {
    let normalized = content.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut excerpt = normalized.chars().take(max_chars).collect::<String>();
    if normalized.chars().count() > max_chars {
        excerpt.push_str("...");
    }
    excerpt
}

fn thread_needs_compaction(
    store: &RuntimeStore,
    thread: &ThreadRecord,
    settings: RuntimeDaemonCompactionSettings,
) -> AppResult<bool> {
    let Some(latest_usage) = store.list_usage(Some(&thread.id), 1)?.into_iter().next() else {
        return Ok(false);
    };
    if latest_usage.total_tokens < settings.threshold_tokens {
        return Ok(false);
    }
    if store.list_turns(&thread.id)?.len() <= settings.keep_tail_turns {
        return Ok(false);
    }

    let events = store.read_events(&thread.id, 0)?;
    let last_usage_seq = events
        .iter()
        .filter(|event| event.kind == "usage_recorded")
        .map(|event| event.seq)
        .max()
        .unwrap_or(0);
    let last_compaction_seq = events
        .iter()
        .filter(|event| event.kind == "thread_compacted")
        .map(|event| event.seq)
        .max()
        .unwrap_or(0);
    Ok(last_usage_seq > last_compaction_seq)
}

fn run_runtime_task_loop(
    config: &AppConfig,
    store: &RuntimeStore,
    task: &TaskRecord,
    thread: &ThreadRecord,
    budget: Option<usize>,
    json: bool,
) -> AppResult<RunResult> {
    let agent = AgentLoop::new(config.clone());
    let session_budget =
        runtime_agent_session_budget(store, &thread.id, config.model.session_budget_microusd)?;
    let approval_resolver: SharedAgentApprovalResolver =
        Rc::new(RefCell::new(RuntimeTaskApprovalResolver {
            store: store.clone(),
            thread_id: thread.id.clone(),
            poll_interval: Duration::from_millis(250),
            max_polls: None,
        }));
    let user_input_resolver: SharedAgentUserInputResolver =
        Rc::new(RefCell::new(RuntimeTaskUserInputResolver {
            store: store.clone(),
            thread_id: thread.id.clone(),
            poll_interval: Duration::from_millis(250),
            max_polls: None,
        }));
    let options = AgentLoopOptions {
        steps: budget.unwrap_or_else(|| AgentLoopOptions::default().steps),
        initial_recent_steps: store.recent_reasoning_replay_entries(&thread.id, 3)?,
        emit_progress: !json,
        persist_session: false,
        session_budget,
        approval_resolver: Some(approval_resolver),
        user_input_resolver: Some(user_input_resolver),
        ..AgentLoopOptions::default()
    };
    agent.run_with(TaskContext::new(task.summary.clone(), None), options)
}

fn runtime_agent_session_budget(
    store: &RuntimeStore,
    thread_id: &str,
    configured_budget_microusd: u64,
) -> AppResult<Option<AgentSessionBudget>> {
    store.ensure_thread_session_budget_microusd(thread_id, configured_budget_microusd)?;
    Ok(store
        .thread_budget_snapshot(thread_id)?
        .map(|snapshot| AgentSessionBudget {
            budget_microusd: snapshot.budget_microusd,
            used_microusd: snapshot.used_microusd,
        }))
}

struct RuntimeTaskApprovalResolver {
    store: RuntimeStore,
    thread_id: String,
    poll_interval: Duration,
    max_polls: Option<usize>,
}

impl AgentApprovalResolver for RuntimeTaskApprovalResolver {
    fn resolve(&mut self, request: &AgentApprovalRequest) -> AppResult<AgentApprovalDecision> {
        let approval = self.store.append_permission_request(
            &self.thread_id,
            None,
            request.tool_name.clone(),
            request.kind.clone(),
            request.target.clone(),
            request.input.clone(),
        )?;
        let mut polls = 0_usize;
        loop {
            for event in self.store.read_events(&self.thread_id, approval.seq)? {
                if let Some(decision) = approval_response_decision(&event, &approval.id) {
                    return Ok(decision);
                }
            }
            polls = polls.saturating_add(1);
            if self.max_polls.is_some_and(|max_polls| polls >= max_polls) {
                return Err(app_error(format!(
                    "timed out waiting for permission response {}",
                    approval.id
                )));
            }
            std::thread::sleep(self.poll_interval);
        }
    }
}

struct RuntimeTaskUserInputResolver {
    store: RuntimeStore,
    thread_id: String,
    poll_interval: Duration,
    max_polls: Option<usize>,
}

impl AgentUserInputResolver for RuntimeTaskUserInputResolver {
    fn resolve(&mut self, request: &AgentUserInputRequest) -> AppResult<AgentUserInputResponse> {
        let raw_questions = request
            .input
            .get("questions")
            .ok_or_else(|| app_error("request_user_input requires `questions`"))?;
        let questions = parse_json_value(raw_questions.trim())
            .map_err(|error| app_error(format!("Invalid request_user_input payload: {error}")))?;
        let user_input = self
            .store
            .append_user_input_request(&self.thread_id, None, questions)?;
        let mut polls = 0_usize;
        loop {
            for event in self.store.read_events(&self.thread_id, user_input.seq)? {
                if let Some(answers) = user_input_response_answers(&event, &user_input.id) {
                    return Ok(AgentUserInputResponse { answers });
                }
            }
            polls = polls.saturating_add(1);
            if self.max_polls.is_some_and(|max_polls| polls >= max_polls) {
                return Err(app_error(format!(
                    "timed out waiting for user input response {}",
                    user_input.id
                )));
            }
            std::thread::sleep(self.poll_interval);
        }
    }
}

fn user_input_response_answers(
    event: &crate::core::runtime::RuntimeEvent,
    request_id: &str,
) -> Option<std::collections::BTreeMap<String, String>> {
    if event.kind != "user_input_response" {
        return None;
    }
    let payload = json_as_object(&event.payload)?;
    let response_request_id = payload.get("request_id").and_then(json_as_string)?;
    if response_request_id != request_id {
        return None;
    }
    let answers = payload.get("answers").and_then(json_as_object)?;
    let answers = answers
        .iter()
        .filter_map(|(key, value)| Some((key.clone(), json_as_string(value)?.to_string())))
        .collect::<std::collections::BTreeMap<_, _>>();
    if answers.is_empty() {
        None
    } else {
        Some(answers)
    }
}

fn approval_response_decision(
    event: &crate::core::runtime::RuntimeEvent,
    request_id: &str,
) -> Option<AgentApprovalDecision> {
    if event.kind != "permission_response" {
        return None;
    }
    let payload = json_as_object(&event.payload)?;
    let response_request_id = payload.get("request_id").and_then(json_as_string)?;
    if response_request_id != request_id {
        return None;
    }
    match payload.get("decision").and_then(json_as_string)? {
        "approved" => Some(AgentApprovalDecision::Approved),
        "denied" => Some(AgentApprovalDecision::Denied),
        _ => None,
    }
}

fn record_runtime_task_result(
    store: &RuntimeStore,
    task: &TaskRecord,
    thread: &ThreadRecord,
    result: &RunResult,
) -> AppResult<String> {
    let user = store.append_turn(&thread.id, "user".to_string(), task.summary.clone())?;
    store.append_item(
        &thread.id,
        Some(&user.id),
        "message".to_string(),
        Some("user".to_string()),
        task.summary.clone(),
        "completed".to_string(),
    )?;
    let message = non_empty_message(&result.final_message);
    let assistant = store.append_turn(&thread.id, "assistant".to_string(), message.clone())?;
    store.append_item(
        &thread.id,
        Some(&assistant.id),
        "message".to_string(),
        Some("assistant".to_string()),
        message.clone(),
        "completed".to_string(),
    )?;
    for event in &result.tool_events {
        store.append_item(
            &thread.id,
            Some(&assistant.id),
            "tool_result".to_string(),
            Some("tool".to_string()),
            format_tool_event(event),
            tool_item_status(event),
        )?;
    }
    let usage_model = result.usage.model.as_deref().unwrap_or(&thread.model);
    let usage = store.append_usage_with_cache(
        &thread.id,
        Some(&assistant.id),
        usage_model.to_string(),
        "runtime_runner".to_string(),
        result.usage.prompt,
        result.usage.completion,
        result.usage.prompt_cache_hit,
        result.usage.prompt_cache_miss,
    )?;
    if !result.prompt_layers.is_empty() {
        store.append_thread_event(
            &thread.id,
            "prompt_layers_recorded",
            prompt_layers_event_payload(&assistant.id, &usage.id, &result.prompt_layers),
        )?;
    }
    store.update_task(&task.id, "completed".to_string(), message)?;
    Ok(assistant.id)
}

fn record_runtime_task_failure(
    store: &RuntimeStore,
    task: &TaskRecord,
    thread: &ThreadRecord,
    error: &str,
) -> AppResult<()> {
    let message = format!("runtime task failed: {error}");
    let assistant = store.append_turn(&thread.id, "assistant".to_string(), message.clone())?;
    store.append_item(
        &thread.id,
        Some(&assistant.id),
        "message".to_string(),
        Some("assistant".to_string()),
        message.clone(),
        "failed".to_string(),
    )?;
    store.update_task(&task.id, "failed".to_string(), message)?;
    Ok(())
}

fn non_empty_message(message: &str) -> String {
    if message.trim().is_empty() {
        "runtime task completed without assistant output".to_string()
    } else {
        message.to_string()
    }
}

fn format_tool_event(event: &ToolEvent) -> String {
    let args = event
        .input
        .iter()
        .map(|(key, value)| format!("{key}={value}"))
        .collect::<Vec<_>>()
        .join(" ");
    if args.is_empty() {
        format!("tool: {}\n{}", event.tool_name, event.output)
    } else {
        format!(
            "tool: {}\nargs: {}\n{}",
            event.tool_name, args, event.output
        )
    }
}

fn tool_item_status(event: &ToolEvent) -> String {
    match event.status {
        ObservationStatus::Ok => "completed".to_string(),
        ObservationStatus::Failed => "failed".to_string(),
    }
}

fn runner_event(
    event_type: &str,
    task_id: &str,
    thread_id: &str,
    runner_id: Option<&str>,
    message: Option<&str>,
) -> JsonValue {
    let mut root = std::collections::BTreeMap::new();
    root.insert(
        "type".to_string(),
        JsonValue::String(event_type.to_string()),
    );
    root.insert(
        "task_id".to_string(),
        JsonValue::String(task_id.to_string()),
    );
    root.insert(
        "thread_id".to_string(),
        JsonValue::String(thread_id.to_string()),
    );
    if let Some(runner_id) = runner_id {
        root.insert(
            "runner_id".to_string(),
            JsonValue::String(runner_id.to_string()),
        );
    }
    if let Some(message) = message {
        root.insert(
            "message".to_string(),
            JsonValue::String(message.to_string()),
        );
    }
    JsonValue::Object(root)
}

fn daemon_tick_event(tick: &RuntimeDaemonTick) -> JsonValue {
    let mut root = std::collections::BTreeMap::new();
    root.insert(
        "type".to_string(),
        JsonValue::String("daemon_tick".to_string()),
    );
    root.insert(
        "triggered_automations".to_string(),
        JsonValue::Number(tick.triggered_automations.to_string()),
    );
    root.insert(
        "executed_tasks".to_string(),
        JsonValue::Number(tick.executed_tasks.to_string()),
    );
    root.insert(
        "executed_rlm_turns".to_string(),
        JsonValue::Number(tick.executed_rlm_turns.to_string()),
    );
    root.insert(
        "recovered_rlm_turns".to_string(),
        JsonValue::Number(tick.recovered_rlm_turns.to_string()),
    );
    root.insert(
        "compacted_threads".to_string(),
        JsonValue::Number(tick.compacted_threads.to_string()),
    );
    root.insert(
        "failed_automations".to_string(),
        JsonValue::Number(tick.failed_automations.to_string()),
    );
    root.insert(
        "failed_tasks".to_string(),
        JsonValue::Number(tick.failed_tasks.to_string()),
    );
    root.insert(
        "failed_rlm_turns".to_string(),
        JsonValue::Number(tick.failed_rlm_turns.to_string()),
    );
    root.insert(
        "failed_rlm_recoveries".to_string(),
        JsonValue::Number(tick.failed_rlm_recoveries.to_string()),
    );
    root.insert(
        "failed_compactions".to_string(),
        JsonValue::Number(tick.failed_compactions.to_string()),
    );
    JsonValue::Object(root)
}

fn daemon_rlm_recovery_event(recovered_count: usize, summary: Option<&str>) -> JsonValue {
    let mut root = std::collections::BTreeMap::new();
    root.insert(
        "type".to_string(),
        JsonValue::String("rlm_recovery_completed".to_string()),
    );
    root.insert(
        "recovered_count".to_string(),
        JsonValue::Number(recovered_count.to_string()),
    );
    root.insert(
        "summary".to_string(),
        summary
            .map(|value| JsonValue::String(value.to_string()))
            .unwrap_or(JsonValue::Null),
    );
    JsonValue::Object(root)
}

fn daemon_rlm_turn_event(
    event_type: &str,
    session_id: &str,
    task_id: &str,
    summary: Option<&str>,
) -> JsonValue {
    let mut root = std::collections::BTreeMap::new();
    root.insert(
        "type".to_string(),
        JsonValue::String(event_type.to_string()),
    );
    root.insert(
        "session_id".to_string(),
        JsonValue::String(session_id.to_string()),
    );
    root.insert(
        "task_id".to_string(),
        JsonValue::String(task_id.to_string()),
    );
    root.insert(
        "summary".to_string(),
        summary
            .map(|value| JsonValue::String(value.to_string()))
            .unwrap_or(JsonValue::Null),
    );
    JsonValue::Object(root)
}

fn daemon_compaction_event(compaction: &ThreadCompactionRecord) -> JsonValue {
    let mut root = std::collections::BTreeMap::new();
    root.insert(
        "type".to_string(),
        JsonValue::String("thread_compacted".to_string()),
    );
    root.insert(
        "thread_id".to_string(),
        JsonValue::String(compaction.thread_id.clone()),
    );
    root.insert(
        "summary_turn_id".to_string(),
        JsonValue::String(compaction.summary_turn.id.clone()),
    );
    root.insert(
        "summarized_turn_count".to_string(),
        JsonValue::Number(compaction.summarized_turn_count.to_string()),
    );
    root.insert(
        "keep_tail_turns".to_string(),
        JsonValue::Number(compaction.keep_tail_turns.to_string()),
    );
    root.insert(
        "summary_source".to_string(),
        JsonValue::String(compaction.summary_source.clone()),
    );
    JsonValue::Object(root)
}

fn daemon_automation_event(automation: &AutomationRecord, task: &TaskRecord) -> JsonValue {
    let mut root = std::collections::BTreeMap::new();
    root.insert(
        "type".to_string(),
        JsonValue::String("automation_triggered".to_string()),
    );
    root.insert(
        "automation_id".to_string(),
        JsonValue::String(automation.id.clone()),
    );
    root.insert("task_id".to_string(), JsonValue::String(task.id.clone()));
    root.insert(
        "next_run_at".to_string(),
        automation
            .next_run_at
            .as_ref()
            .map(|value| JsonValue::String(value.clone()))
            .unwrap_or(JsonValue::Null),
    );
    JsonValue::Object(root)
}

fn daemon_error_event(event_type: &str, id: &str, message: &str) -> JsonValue {
    let mut root = std::collections::BTreeMap::new();
    root.insert(
        "type".to_string(),
        JsonValue::String(event_type.to_string()),
    );
    root.insert("id".to_string(), JsonValue::String(id.to_string()));
    root.insert(
        "message".to_string(),
        JsonValue::String(message.to_string()),
    );
    JsonValue::Object(root)
}

fn automation_is_due(automation: &AutomationRecord, now_secs: u64) -> bool {
    automation.status == "active"
        && automation
            .next_run_at
            .as_deref()
            .and_then(parse_epoch_seconds)
            .is_some_and(|next_run_at| next_run_at <= now_secs)
}

fn next_run_for_schedule(schedule: &str, now_secs: u64) -> Option<String> {
    parse_schedule_interval_seconds(schedule)
        .map(|interval| format_epoch_seconds(now_secs.saturating_add(interval)))
}

fn parse_schedule_interval_seconds(schedule: &str) -> Option<u64> {
    let lower = schedule.trim().to_ascii_lowercase();
    if matches!(lower.as_str(), "" | "manual" | "once" | "@once") {
        return None;
    }
    let token = lower
        .strip_prefix("@every ")
        .or_else(|| lower.strip_prefix("every "))
        .or_else(|| lower.strip_prefix("every:"))
        .or_else(|| lower.strip_prefix("interval "))
        .or_else(|| lower.strip_prefix("interval:"))
        .unwrap_or(&lower)
        .split_whitespace()
        .next()
        .unwrap_or("");
    parse_duration_seconds(token)
}

fn parse_duration_seconds(token: &str) -> Option<u64> {
    let split = token
        .char_indices()
        .find(|(_, ch)| !ch.is_ascii_digit())
        .map(|(index, _)| index)
        .unwrap_or(token.len());
    let (digits, unit) = token.split_at(split);
    let value = digits.parse::<u64>().ok()?;
    if value == 0 {
        return None;
    }
    let multiplier = match unit {
        "" | "s" | "sec" | "secs" | "second" | "seconds" => 1,
        "m" | "min" | "mins" | "minute" | "minutes" => 60,
        "h" | "hr" | "hrs" | "hour" | "hours" => 60 * 60,
        "d" | "day" | "days" => 24 * 60 * 60,
        _ => return None,
    };
    value.checked_mul(multiplier)
}

fn parse_epoch_seconds(value: &str) -> Option<u64> {
    value
        .strip_prefix("epoch+")
        .unwrap_or(value)
        .parse::<u64>()
        .ok()
}

fn format_epoch_seconds(value: u64) -> String {
    format!("epoch+{value}")
}

fn current_epoch_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

fn run_subagent_fixture_smoke(json: bool) -> AppResult<()> {
    let root = subagent_fixture_temp_root();
    let report = crate::tools::dispatch_subagent::run_subagent_fixture_smoke_at(&root)?;
    if json {
        println!("{}", render_subagent_fixture_smoke_json(&report));
    } else {
        print_subagent_fixture_smoke_report(&report);
    }
    Ok(())
}

fn subagent_fixture_temp_root() -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    std::env::temp_dir().join(format!(
        "deepseek-subagent-fixture-{}-{nanos}",
        std::process::id()
    ))
}

fn render_subagent_fixture_smoke_json(
    report: &crate::tools::dispatch_subagent::SubagentFixtureSmokeReport,
) -> String {
    format!(
        "{{\"kind\":\"deepseek.subagent_fixture_smoke.v1\",\"workdir\":\"{}\",\"parser_ok\":{},\"disjoint_write_scope_ok\":{},\"readback_required_ok\":{},\"blocker_summary_ok\":{},\"conflict_summary_ok\":{},\"artifact_ok\":{},\"child_count\":{}}}",
        json_escape(&report.workdir.display().to_string()),
        report.parser_ok,
        report.disjoint_write_scope_ok,
        report.readback_required_ok,
        report.blocker_summary_ok,
        report.conflict_summary_ok,
        report.artifact_ok,
        report.child_count
    )
}

fn print_subagent_fixture_smoke_report(
    report: &crate::tools::dispatch_subagent::SubagentFixtureSmokeReport,
) {
    println!("Subagent fixture smoke: ok");
    println!("workdir: {}", report.workdir.display());
    println!(
        "parser={} disjoint_write_scope={} readback_required={}",
        report.parser_ok, report.disjoint_write_scope_ok, report.readback_required_ok
    );
    println!(
        "blocker_summary={} conflict_summary={} artifact={} child_count={}",
        report.blocker_summary_ok,
        report.conflict_summary_ok,
        report.artifact_ok,
        report.child_count
    );
}

fn list_threads(config_dir: &str) -> AppResult<()> {
    let dir = agent_threads_dir(config_dir);
    let active = read_active_thread(config_dir).unwrap_or_default();
    let Ok(entries) = std::fs::read_dir(&dir) else {
        println!("No subagent threads recorded.");
        return Ok(());
    };
    let mut threads = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|value| value.to_str()) == Some("md"))
        .collect::<Vec<_>>();
    threads.sort();
    if threads.is_empty() {
        println!("No subagent threads recorded.");
        return Ok(());
    }

    println!("Subagent threads:");
    for path in threads {
        let id = path
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or("-");
        let marker = if id == active { "*" } else { "-" };
        let title = std::fs::read_to_string(&path)
            .ok()
            .and_then(|body| body.lines().next().map(str::to_string))
            .unwrap_or_else(|| "# Agent Thread".to_string());
        println!("{marker} {id}: {}", title.trim_start_matches("# "));
    }
    Ok(())
}

fn show_thread(config_dir: &str, id: &str) -> AppResult<()> {
    let path = valid_thread_path(config_dir, id)?;
    let body = std::fs::read_to_string(&path)
        .map_err(|error| app_error(format!("failed to read thread {}: {error}", path.display())))?;
    println!("{body}");
    Ok(())
}

fn switch_thread(config_dir: &str, id: &str) -> AppResult<()> {
    let path = valid_thread_path(config_dir, id)?;
    let active = active_agent_thread_path(config_dir);
    if let Some(parent) = active.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&active, id)?;
    println!("active subagent thread: {id}");
    println!("source: {}", path.display());
    Ok(())
}

fn current_thread(config_dir: &str) -> AppResult<()> {
    match read_active_thread(config_dir) {
        Some(id) => {
            println!("active subagent thread: {id}");
            if let Some(path) = thread_file_path(config_dir, &id) {
                println!("source: {}", path.display());
            }
        }
        None => println!("No active subagent thread."),
    }
    Ok(())
}

fn clear_thread(config_dir: &str) -> AppResult<()> {
    let active = active_agent_thread_path(config_dir);
    if active.exists() {
        std::fs::remove_file(&active)?;
    }
    println!("active subagent thread cleared");
    Ok(())
}

fn valid_thread_path(config_dir: &str, id: &str) -> AppResult<std::path::PathBuf> {
    if !validate_thread_id(id) {
        return Err(app_error("invalid subagent thread id"));
    }
    let path =
        thread_file_path(config_dir, id).ok_or_else(|| app_error("invalid subagent thread id"))?;
    if !path.is_file() {
        return Err(app_error(format!("subagent thread `{id}` not found")));
    }
    Ok(path)
}

fn read_active_thread(config_dir: &str) -> Option<String> {
    std::fs::read_to_string(active_agent_thread_path(config_dir))
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| validate_thread_id(value))
}

#[allow(dead_code)]
fn only_valid(results: Vec<AgentLoadResult>) -> Vec<crate::core::agents::AgentSpec> {
    results.into_iter().filter_map(Result::ok).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_valid_filters_invalid_results() {
        let results = vec![
            Ok(crate::core::agents::AgentSpec {
                name: "reviewer".to_string(),
                description: "Reviews code".to_string(),
                tools: Vec::new(),
                model: None,
                prompt: "Review.".to_string(),
                path: ".dscode/agents/reviewer.md".into(),
                source: AgentSource::Project,
            }),
            Err(crate::core::agents::AgentLoadError {
                path: ".dscode/agents/bad.md".into(),
                message: "bad".to_string(),
            }),
        ];

        let valid = only_valid(results);

        assert_eq!(valid.len(), 1);
        assert_eq!(valid[0].name, "reviewer");
    }

    #[test]
    fn agents_shell_cli_args_build_protocol_requests() {
        let start = agents_shell_request_json(&AgentsShellArgs {
            action: AgentsShellAction::Start {
                command: "echo hello".to_string(),
                cwd: Some("subdir".to_string()),
                tty: true,
                tty_rows: Some(33),
                tty_cols: Some(101),
            },
            json: true,
        });
        let start = json_as_object(&start).unwrap();
        assert_eq!(start.get("method").and_then(json_as_string), Some("start"));
        assert_eq!(
            start.get("command").and_then(json_as_string),
            Some("echo hello")
        );
        assert_eq!(start.get("cwd").and_then(json_as_string), Some("subdir"));
        assert!(matches!(start.get("tty"), Some(JsonValue::Bool(true))));
        assert_eq!(start.get("tty_rows").and_then(json_as_u64), Some(33));
        assert_eq!(start.get("tty_cols").and_then(json_as_u64), Some(101));

        let stdin = agents_shell_request_json(&AgentsShellArgs {
            action: AgentsShellAction::Stdin {
                task_id: "task-1".to_string(),
                input: Some("probe\n".to_string()),
                close_stdin: true,
                timeout_ms: Some(100),
            },
            json: false,
        });
        let stdin = json_as_object(&stdin).unwrap();
        assert_eq!(stdin.get("method").and_then(json_as_string), Some("stdin"));
        assert_eq!(
            stdin.get("task_id").and_then(json_as_string),
            Some("task-1")
        );
        assert_eq!(stdin.get("input").and_then(json_as_string), Some("probe\n"));
        assert!(matches!(
            stdin.get("close_stdin"),
            Some(JsonValue::Bool(true))
        ));
        assert_eq!(stdin.get("timeout_ms").and_then(json_as_u64), Some(100));

        let cancel = agents_shell_request_json(&AgentsShellArgs {
            action: AgentsShellAction::Cancel {
                task_id: None,
                all: true,
            },
            json: false,
        });
        let cancel = json_as_object(&cancel).unwrap();
        assert_eq!(
            cancel.get("method").and_then(json_as_string),
            Some("cancel")
        );
        assert!(matches!(cancel.get("all"), Some(JsonValue::Bool(true))));

        let attach_follow = AgentsShellArgs {
            action: AgentsShellAction::Attach {
                task_id: "task-2".to_string(),
                cursor: Some(9),
                wait_ms: Some(250),
                limit_bytes: Some(4096),
                tail: false,
                follow: true,
                interactive: false,
                raw: true,
                poll_ms: Some(50),
                max_ms: Some(1000),
            },
            json: false,
        };
        assert!(agents_shell_attach_follow_requested(&attach_follow));
        assert!(agents_shell_attach_raw_requested(&attach_follow));
        let attach = agents_shell_request_json(&attach_follow);
        let attach = json_as_object(&attach).unwrap();
        assert_eq!(
            attach.get("method").and_then(json_as_string),
            Some("attach")
        );
        assert_eq!(
            attach.get("task_id").and_then(json_as_string),
            Some("task-2")
        );
        assert_eq!(attach.get("cursor").and_then(json_as_u64), Some(9));
        assert_eq!(attach.get("wait_ms").and_then(json_as_u64), Some(250));
        assert_eq!(attach.get("limit_bytes").and_then(json_as_u64), Some(4096));
        assert!(!attach.contains_key("follow"));
        assert!(!attach.contains_key("raw"));
        assert!(!attach.contains_key("poll_ms"));
        assert!(!attach.contains_key("max_ms"));

        let stream_request = shell_attach_stream_request_json(&ShellAttachStreamFollow {
            cwd: Path::new("."),
            task_id: "task-2",
            cursor: Some(9),
            wait_ms: Some(250),
            limit_bytes: Some(4096),
            tail: false,
            raw: true,
            poll_ms: Some(50),
            max_ms: Some(1000),
            json: false,
        });
        let stream_request = json_as_object(&stream_request).unwrap();
        assert_eq!(
            stream_request.get("method").and_then(json_as_string),
            Some("attach_stream")
        );
        assert_eq!(
            stream_request.get("task_id").and_then(json_as_string),
            Some("task-2")
        );
        assert_eq!(
            stream_request.get("poll_ms").and_then(json_as_u64),
            Some(50)
        );
        assert_eq!(
            stream_request.get("max_ms").and_then(json_as_u64),
            Some(1000)
        );

        let byte_stream = agents_shell_request_json(&AgentsShellArgs {
            action: AgentsShellAction::ByteStream {
                task_id: "task-3".to_string(),
                cursor: Some(11),
                wait_ms: Some(125),
                limit_bytes: Some(2048),
                tail: true,
                input: Some("probe\n".to_string()),
                close_stdin: false,
                tty_rows: Some(33),
                tty_cols: Some(101),
                poll_ms: Some(25),
                max_ms: Some(500),
                max_events: Some(4),
                raw_proxy: true,
                terminal_proxy: false,
            },
            json: true,
        });
        let byte_stream = json_as_object(&byte_stream).unwrap();
        assert!(agents_shell_byte_stream_requested(&AgentsShellArgs {
            action: AgentsShellAction::ByteStream {
                task_id: "task-3".to_string(),
                cursor: None,
                wait_ms: None,
                limit_bytes: None,
                tail: false,
                input: None,
                close_stdin: false,
                tty_rows: None,
                tty_cols: None,
                poll_ms: None,
                max_ms: None,
                max_events: None,
                raw_proxy: false,
                terminal_proxy: false,
            },
            json: false,
        }));
        assert_eq!(
            byte_stream.get("method").and_then(json_as_string),
            Some("byte_stream")
        );
        assert_eq!(
            byte_stream.get("task_id").and_then(json_as_string),
            Some("task-3")
        );
        assert_eq!(byte_stream.get("cursor").and_then(json_as_u64), Some(11));
        assert_eq!(byte_stream.get("wait_ms").and_then(json_as_u64), Some(125));
        assert_eq!(
            byte_stream.get("limit_bytes").and_then(json_as_u64),
            Some(2048)
        );
        assert_eq!(
            byte_stream.get("input").and_then(json_as_string),
            Some("probe\n")
        );
        assert_eq!(byte_stream.get("tty_rows").and_then(json_as_u64), Some(33));
        assert_eq!(byte_stream.get("tty_cols").and_then(json_as_u64), Some(101));
        assert_eq!(byte_stream.get("poll_ms").and_then(json_as_u64), Some(25));
        assert_eq!(byte_stream.get("max_ms").and_then(json_as_u64), Some(500));
        assert_eq!(byte_stream.get("max_events").and_then(json_as_u64), Some(4));
        assert!(matches!(
            byte_stream.get("raw_proxy"),
            Some(JsonValue::Bool(true))
        ));

        let fd_proxy = agents_shell_request_json(&AgentsShellArgs {
            action: AgentsShellAction::FdProxy {
                task_id: "task-4".to_string(),
                tty_rows: Some(36),
                tty_cols: Some(104),
                max_ms: Some(1500),
            },
            json: false,
        });
        let fd_proxy = json_as_object(&fd_proxy).unwrap();
        assert!(agents_shell_fd_proxy_requested(&AgentsShellArgs {
            action: AgentsShellAction::FdProxy {
                task_id: "task-4".to_string(),
                tty_rows: None,
                tty_cols: None,
                max_ms: None,
            },
            json: false,
        }));
        assert_eq!(
            fd_proxy.get("method").and_then(json_as_string),
            Some("pty_fd")
        );
        assert_eq!(
            fd_proxy.get("task_id").and_then(json_as_string),
            Some("task-4")
        );
        assert_eq!(fd_proxy.get("tty_rows").and_then(json_as_u64), Some(36));
        assert_eq!(fd_proxy.get("tty_cols").and_then(json_as_u64), Some(104));
        assert_eq!(fd_proxy.get("max_ms").and_then(json_as_u64), Some(1500));
    }

    #[test]
    fn agents_shell_attach_follow_parses_cursor_status_and_payload() {
        let stdout_summary =
            "task_id: task-1\nstatus: running\nnext_offset: 12\nterminal:\nhello\n";
        assert_eq!(shell_attach_summary_next_cursor(stdout_summary), Some(12));
        assert_eq!(
            shell_attach_summary_terminal_payload(stdout_summary),
            Some("hello")
        );
        assert_eq!(
            shell_summary_value(stdout_summary, "status"),
            Some("running")
        );

        let terminal_summary =
            "task_id: task-2\nstatus: completed\nnext_cursor: 3\nevents: 1\nterminal:\n[3 output epoch] done\n";
        assert_eq!(shell_attach_summary_next_cursor(terminal_summary), Some(3));
        assert_eq!(
            shell_attach_summary_terminal_payload(terminal_summary),
            Some("[3 output epoch] done")
        );
        assert_eq!(
            shell_summary_value(terminal_summary, "status"),
            Some("completed")
        );

        let raw_summary = "task_id: task-raw\nstatus: running\nnext_cursor: 4\nevents: 1\nterminal_raw_base64:\n4 output G1szMW1oaQo=\nterminal:\n[4 output epoch] hi\n";
        assert_eq!(
            shell_summary_section_payload(raw_summary, "terminal_raw_base64"),
            Some("4 output G1szMW1oaQo=")
        );
        assert_eq!(
            shell_attach_summary_terminal_payload(raw_summary),
            Some("[4 output epoch] hi")
        );
        let mut raw = Vec::new();
        assert!(shell_attach_write_terminal_payload(raw_summary, &mut raw, true, false).unwrap());
        assert_eq!(raw, b"\x1b[31mhi\n");
        let raw_outputs_json = shell_attach_raw_outputs_json_from_summary(raw_summary).unwrap();
        let raw_outputs = json_as_array(&raw_outputs_json).unwrap();
        assert_eq!(raw_outputs.len(), 1);
        let raw_output = json_as_object(&raw_outputs[0]).unwrap();
        assert_eq!(raw_output.get("seq").and_then(json_as_u64), Some(4));
        assert_eq!(
            raw_output.get("raw_base64").and_then(json_as_string),
            Some("G1szMW1oaQo=")
        );
        let event_preview_summary = "task_id: task-raw\nstatus: running\nmode: terminal_event_attach\nnext_cursor: 5\nevents: 1\nterminal:\n[5 output epoch] preview-only\n";
        let structured_response =
            BTreeMap::from([("terminal_raw_outputs".to_string(), raw_outputs_json.clone())]);
        let mut structured_raw = Vec::new();
        assert!(shell_attach_write_response_terminal_payload(
            &structured_response,
            event_preview_summary,
            &mut structured_raw,
            true,
            true
        )
        .unwrap());
        assert_eq!(structured_raw, b"\x1b[31mhi\n");
        let mut preview_only = Vec::new();
        assert!(!shell_attach_write_terminal_payload(
            event_preview_summary,
            &mut preview_only,
            true,
            true
        )
        .unwrap());
        assert!(preview_only.is_empty());

        let replay_summary = "task_id: task-3\nstatus: running\nstream: stdout\noffset: 0\nnext_offset: 5\ndata:\nhello\n";
        assert_eq!(
            shell_summary_section_payload(replay_summary, "data"),
            Some("hello")
        );
        assert_eq!(
            shell_summary_value(replay_summary, "next_offset"),
            Some("5")
        );
        assert_eq!(decode_shell_base64("").unwrap(), Vec::<u8>::new());
        assert_eq!(decode_shell_base64("Zg==").unwrap(), b"f".to_vec());
        assert_eq!(decode_shell_base64("Zm8=").unwrap(), b"fo".to_vec());
        assert_eq!(decode_shell_base64("Zm9v").unwrap(), b"foo".to_vec());
    }

    #[test]
    fn agents_shell_attach_interactive_maps_terminal_keys() {
        assert!(agents_shell_attach_interactive_requested(
            &AgentsShellArgs {
                action: AgentsShellAction::Attach {
                    task_id: "task-1".to_string(),
                    cursor: None,
                    wait_ms: None,
                    limit_bytes: None,
                    tail: false,
                    follow: false,
                    interactive: true,
                    raw: false,
                    poll_ms: None,
                    max_ms: None,
                },
                json: false,
            }
        ));
        assert_eq!(
            shell_attach_interactive_key_input(KeyEvent::new(
                KeyCode::Char('x'),
                KeyModifiers::NONE
            )),
            Some("x".to_string())
        );
        assert_eq!(
            shell_attach_interactive_key_input(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            Some("\r".to_string())
        );
        assert_eq!(
            shell_attach_interactive_key_input(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE)),
            Some("\x1b[A".to_string())
        );
        assert_eq!(
            shell_attach_interactive_key_input(KeyEvent::new(
                KeyCode::Char('c'),
                KeyModifiers::CONTROL,
            )),
            Some("\x03".to_string())
        );
        assert!(shell_attach_interactive_is_detach_key(KeyEvent::new(
            KeyCode::Char(']'),
            KeyModifiers::CONTROL,
        )));
    }

    #[test]
    fn thread_commands_switch_and_clear_active_thread() {
        let root = temp_root("threads");
        let config_dir = root.join(".dscode");
        let threads = agent_threads_dir(config_dir.to_str().unwrap());
        std::fs::create_dir_all(&threads).unwrap();
        std::fs::write(threads.join("thread-1.md"), "# Agent Thread thread-1\n").unwrap();

        switch_thread(config_dir.to_str().unwrap(), "thread-1").unwrap();
        assert_eq!(
            read_active_thread(config_dir.to_str().unwrap()).as_deref(),
            Some("thread-1")
        );

        current_thread(config_dir.to_str().unwrap()).unwrap();
        clear_thread(config_dir.to_str().unwrap()).unwrap();
        assert!(read_active_thread(config_dir.to_str().unwrap()).is_none());
    }

    #[test]
    fn show_thread_rejects_unsafe_id() {
        let root = temp_root("unsafe-thread");
        let config_dir = root.join(".dscode");

        let error = show_thread(config_dir.to_str().unwrap(), "../bad").unwrap_err();

        assert!(error.to_string().contains("invalid subagent thread id"));
    }

    #[test]
    fn rlm_cli_read_lifecycle_args_build_tool_inputs() {
        let status = rlm_status_tool_input(&AgentsRlmStatusArgs {
            session_id: Some("live.1".to_string()),
            limit: Some(5),
            json: true,
        });
        assert_eq!(status.get("session_id"), Some("live.1"));
        assert_eq!(status.get("limit"), Some("5"));

        let events = rlm_events_tool_input(&AgentsRlmEventsArgs {
            session_id: "live.1".to_string(),
            cursor: Some(7),
            limit: Some(3),
            json: false,
        });
        assert_eq!(events.get("session_id"), Some("live.1"));
        assert_eq!(events.get("cursor"), Some("7"));
        assert_eq!(events.get("limit"), Some("3"));

        let wait = rlm_wait_tool_input(&AgentsRlmWaitArgs {
            session_id: "live.1".to_string(),
            cursor: Some(9),
            limit: Some(4),
            timeout_ms: Some(2500),
            poll_interval_ms: Some(50),
            json: true,
        });
        assert_eq!(wait.get("session_id"), Some("live.1"));
        assert_eq!(wait.get("cursor"), Some("9"));
        assert_eq!(wait.get("limit"), Some("4"));
        assert_eq!(wait.get("timeout_ms"), Some("2500"));
        assert_eq!(wait.get("poll_interval_ms"), Some("50"));
    }

    #[test]
    fn rlm_cli_stateful_lifecycle_args_build_tool_inputs() {
        let cancel = rlm_cancel_tool_input(&AgentsRlmCancelArgs {
            session_id: "live.1".to_string(),
            task_id: Some("task-1".to_string()),
            all: false,
            force: true,
            reason: Some("operator stop".to_string()),
            json: true,
        });
        assert_eq!(cancel.get("session_id"), Some("live.1"));
        assert_eq!(cancel.get("task_id"), Some("task-1"));
        assert_eq!(cancel.get("force"), Some("true"));
        assert_eq!(cancel.get("reason"), Some("operator stop"));

        let recover = rlm_recover_tool_input(&AgentsRlmRecoverArgs {
            session_id: None,
            all: true,
            mode: Some("fail".to_string()),
            dry_run: true,
            force: true,
            limit: Some(8),
            reason: Some("takeover".to_string()),
            json: false,
        });
        assert_eq!(recover.get("all"), Some("true"));
        assert_eq!(recover.get("mode"), Some("fail"));
        assert_eq!(recover.get("dry_run"), Some("true"));
        assert_eq!(recover.get("force"), Some("true"));
        assert_eq!(recover.get("limit"), Some("8"));
        assert_eq!(recover.get("reason"), Some("takeover"));

        let stop = rlm_stop_tool_input(&AgentsRlmStopArgs {
            session_id: "live.1".to_string(),
            reason: Some("done".to_string()),
            json: false,
        });
        assert_eq!(stop.get("session_id"), Some("live.1"));
        assert_eq!(stop.get("reason"), Some("done"));

        let run_next = rlm_run_next_tool_input(&AgentsRlmRunNextArgs {
            session_id: "live.1".to_string(),
            task_id: Some("task-2".to_string()),
            dry_run: true,
            json: false,
        });
        assert_eq!(run_next.get("session_id"), Some("live.1"));
        assert_eq!(run_next.get("task_id"), Some("task-2"));
        assert_eq!(run_next.get("dry_run"), Some("true"));

        let drain = rlm_drain_tool_input(&AgentsRlmDrainArgs {
            session_id: "live.1".to_string(),
            max_turns: Some(4),
            dry_run: true,
            json: true,
        });
        assert_eq!(drain.get("session_id"), Some("live.1"));
        assert_eq!(drain.get("max_turns"), Some("4"));
        assert_eq!(drain.get("dry_run"), Some("true"));
    }

    #[test]
    fn shell_supervisor_protocol_parses_methods_and_defaults_to_health() {
        assert_eq!(parse_shell_supervisor_method("").unwrap(), "health");
        assert_eq!(
            parse_shell_supervisor_method(r#"{"method":"status"}"#).unwrap(),
            "status"
        );
        assert!(parse_shell_supervisor_method("[]")
            .unwrap_err()
            .to_string()
            .contains("must be a JSON object"));
    }

    #[test]
    fn shell_supervisor_tcp_endpoint_parser_accepts_loopback_only() {
        assert_eq!(
            shell_supervisor_tcp_endpoint_from_label("tcp://127.0.0.1:43210").unwrap(),
            "127.0.0.1:43210"
        );
        assert_eq!(
            shell_supervisor_tcp_endpoint_from_label("127.0.0.1:43210").unwrap(),
            "127.0.0.1:43210"
        );
        assert!(
            shell_supervisor_tcp_endpoint_from_label("tcp://192.0.2.10:43210")
                .unwrap_err()
                .to_string()
                .contains("loopback")
        );
    }

    #[cfg(windows)]
    #[test]
    fn shell_supervisor_windows_tcp_daemon_client_smoke() {
        let root = temp_root("shell-supervisor-windows-tcp");
        std::fs::create_dir_all(&root).unwrap();
        let daemon_root = root.clone();
        let handle = std::thread::spawn(move || {
            run_shell_supervisor_daemon(&daemon_root, false).map_err(|error| error.to_string())
        });

        let endpoint = {
            let deadline = Instant::now() + Duration::from_millis(5_000);
            loop {
                match read_shell_supervisor_tcp_endpoint(&root).and_then(|endpoint| {
                    shell_supervisor_tcp_request(&endpoint, "health").map(|_| endpoint)
                }) {
                    Ok(endpoint) => break endpoint,
                    Err(error) if Instant::now() < deadline => {
                        let _ = error;
                        std::thread::sleep(Duration::from_millis(50));
                    }
                    Err(error) => panic!("timed out waiting for Windows TCP supervisor: {error}"),
                }
            }
        };

        let smoke = shell_supervisor_windows_tcp_control_smoke(&endpoint, 5_000);
        let mut installed_report = ServiceSmokeReport {
            kind: AgentsServiceKind::All,
            installed: true,
            binary: "deepseek".to_string(),
            workdir: root.clone(),
            requested_addr: "127.0.0.1:8765".to_string(),
            resolved_addr: "127.0.0.1:8765".to_string(),
            addr_error: None,
            timeout_ms: 5_000,
            checks: Vec::new(),
        };
        service_smoke_check_installed_shell_supervisor(&mut installed_report);
        let _ = shell_supervisor_tcp_request(&endpoint, "shutdown");
        let daemon = handle
            .join()
            .expect("Windows TCP supervisor thread panicked");
        assert!(daemon.is_ok(), "Windows TCP supervisor failed: {daemon:?}");
        smoke.unwrap();
        assert!(
            installed_report
                .checks
                .iter()
                .any(|check| check.status == ServiceDoctorStatus::Ok
                    && check.name == "installed_shell_supervisor_control"),
            "{:?}",
            installed_report.checks
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn shell_supervisor_protocol_reports_unsupported_unknown_method() {
        let response = shell_supervisor_protocol_response(
            "native_pty",
            Path::new("/work/repo"),
            Path::new("/work/repo/.dscode/shell-supervisor/supervisor.sock"),
            "epoch+1",
        );
        let object = json_as_object(&response).unwrap();

        assert_eq!(
            json_as_string(object.get("status").unwrap()),
            Some("unsupported")
        );
        assert_eq!(
            json_as_string(object.get("pty_backend").unwrap()),
            Some("none")
        );
        assert!(matches!(
            object.get("native_pty"),
            Some(JsonValue::Bool(value)) if *value == native_supervisor_pty_supported()
        ));
        assert!(matches!(
            object.get("active_jobs"),
            Some(JsonValue::Number(value)) if value == "0"
        ));
        assert!(json_as_string(object.get("error").unwrap())
            .unwrap()
            .contains("is not supported by this protocol"));
    }

    #[test]
    fn shell_supervisor_protocol_start_creates_durable_job() {
        let root = temp_root("shell-supervisor-start");
        let state_dir = root.join(".dscode/shell-supervisor");
        std::fs::create_dir_all(&state_dir).unwrap();
        let request = parse_shell_supervisor_request(
            r#"{"method":"start","arguments":{"command":"tail -f /dev/null","tty":false}}"#,
        )
        .unwrap();

        let response = shell_supervisor_protocol_response_for_request(
            &request,
            &root,
            &state_dir.join("supervisor.sock"),
            "epoch+start",
        );
        let object = json_as_object(&response).unwrap();
        let task_id = json_as_string(object.get("task_id").unwrap()).unwrap();
        let manifest = std::fs::read_to_string(
            root.join(".dscode/shell-jobs")
                .join(task_id)
                .join("manifest.json"),
        )
        .unwrap();
        let supervisor_manifest = std::fs::read_to_string(state_dir.join("manifest.json")).unwrap();

        assert_eq!(json_as_string(object.get("status").unwrap()), Some("ok"));
        assert_eq!(json_as_string(object.get("method").unwrap()), Some("start"));
        assert_eq!(
            json_as_string(object.get("job_pty_backend").unwrap()),
            Some("none")
        );
        assert!(matches!(
            object.get("job_tty"),
            Some(JsonValue::Bool(false))
        ));
        assert!(matches!(
            object.get("native_pty"),
            Some(JsonValue::Bool(value)) if *value == native_supervisor_pty_supported()
        ));
        assert!(matches!(
            object.get("active_jobs"),
            Some(JsonValue::Number(value)) if value == "1"
        ));
        assert!(
            manifest.contains(r#""command":"tail -f /dev/null""#),
            "{manifest}"
        );
        assert!(
            supervisor_manifest
                .contains(r#""methods":["health","status","show","start","wait","replay","attach","attach_stream","byte_stream","pty_fd","stdin","resize","cancel","shutdown"]"#),
            "{supervisor_manifest}"
        );
        assert!(
            supervisor_manifest.contains(r#""unsupported_methods":[]"#),
            "{supervisor_manifest}"
        );
        assert!(
            supervisor_manifest.contains(r#""active_jobs":1"#),
            "{supervisor_manifest}"
        );

        let _ = crate::tools::exec_shell::ExecShellCancelTool.execute(
            ToolInput::new()
                .with_arg("cwd", root.display().to_string())
                .with_arg("task_id", task_id.to_string()),
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn shell_supervisor_protocol_tty_start_records_native_pty_events() {
        if !native_supervisor_pty_supported() {
            return;
        }
        let root = temp_root("shell-supervisor-native-pty");
        let state_dir = root.join(".dscode/shell-supervisor");
        std::fs::create_dir_all(&state_dir).unwrap();
        let socket = state_dir.join("supervisor.sock");
        let start = parse_shell_supervisor_request(
            r#"{"method":"start","arguments":{"command":"echo native-pty-ready","tty":true,"tty_rows":24,"tty_cols":80}}"#,
        )
        .unwrap();

        let response =
            shell_supervisor_protocol_response_for_request(&start, &root, &socket, "epoch+native");
        let object = json_as_object(&response).unwrap();
        assert_eq!(
            json_as_string(object.get("status").unwrap()),
            Some("ok"),
            "{response:?}"
        );
        let task_id = json_as_string(object.get("task_id").unwrap())
            .unwrap()
            .to_string();
        assert_eq!(
            json_as_string(object.get("job_pty_backend").unwrap()),
            Some("native-supervisor")
        );
        assert!(matches!(object.get("job_tty"), Some(JsonValue::Bool(true))));
        #[cfg(windows)]
        {
            let stdin = parse_shell_supervisor_request(&format!(
                r#"{{"method":"stdin","arguments":{{"task_id":"{task_id}","input_base64":"G1sxOzFS","timeout_ms":1000}}}}"#
            ))
            .unwrap();
            let _ = shell_supervisor_protocol_response_for_request(
                &stdin,
                &root,
                &socket,
                "epoch+cursor-response",
            );
        }

        let wait = parse_shell_supervisor_request(&format!(
            r#"{{"method":"wait","arguments":{{"task_id":"{task_id}","timeout_ms":2000}}}}"#
        ))
        .unwrap();
        let response =
            shell_supervisor_protocol_response_for_request(&wait, &root, &socket, "epoch+wait");
        let object = json_as_object(&response).unwrap();
        let wait_summary = json_as_string(object.get("wait_summary").unwrap()).unwrap();
        assert!(wait_summary.contains("pty_backend: native-supervisor"));
        assert!(wait_summary.contains("attachable: true"));

        let manifest = std::fs::read_to_string(
            root.join(".dscode/shell-jobs")
                .join(&task_id)
                .join("manifest.json"),
        )
        .unwrap();
        assert!(manifest.contains(r#""pty_backend":"native-supervisor""#));
        assert!(manifest.contains(r#""terminal_event_log":"terminal-events.jsonl""#));

        let mut replay_summary = String::new();
        for _ in 0..20 {
            let replay = parse_shell_supervisor_request(&format!(
                r#"{{"method":"replay","arguments":{{"task_id":"{task_id}","stream":"terminal","cursor":0,"limit_bytes":4000}}}}"#
            ))
            .unwrap();
            let response = shell_supervisor_protocol_response_for_request(
                &replay,
                &root,
                &socket,
                "epoch+replay",
            );
            let object = json_as_object(&response).unwrap();
            replay_summary = json_as_string(object.get("replay_summary").unwrap())
                .unwrap()
                .to_string();
            if replay_summary.contains("native-pty-ready") {
                break;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        assert!(
            replay_summary.contains("stream: terminal"),
            "{replay_summary}"
        );
        assert!(
            replay_summary.contains("native-pty-ready"),
            "{replay_summary}"
        );

        let mut attach_summary = String::new();
        let mut attach_raw_outputs = 0usize;
        for _ in 0..20 {
            let attach = parse_shell_supervisor_request(&format!(
                r#"{{"method":"attach","arguments":{{"task_id":"{task_id}","cursor":0,"limit_bytes":4000}}}}"#
            ))
            .unwrap();
            let response = shell_supervisor_protocol_response_for_request(
                &attach,
                &root,
                &socket,
                "epoch+attach",
            );
            let object = json_as_object(&response).unwrap();
            attach_summary = json_as_string(object.get("attach_summary").unwrap())
                .unwrap()
                .to_string();
            attach_raw_outputs = object
                .get("terminal_raw_outputs")
                .and_then(json_as_array)
                .map(Vec::len)
                .unwrap_or(0);
            if attach_summary.contains("native-pty-ready") && attach_raw_outputs > 0 {
                break;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        assert!(
            attach_summary.contains("terminal_raw_base64"),
            "{attach_summary}"
        );
        assert!(
            attach_summary.contains("native-pty-ready"),
            "{attach_summary}"
        );
        assert!(
            attach_raw_outputs > 0,
            "attach response should expose structured terminal_raw_outputs: {attach_summary}"
        );

        #[cfg(unix)]
        {
            let (mut client, server) = std::os::unix::net::UnixStream::pair().unwrap();
            client
                .write_all(
                    format!(
                        r#"{{"method":"attach_stream","arguments":{{"task_id":"{task_id}","cursor":0,"limit_bytes":4000,"max_ms":500,"max_events":3,"poll_ms":25}}}}"#
                    )
                    .as_bytes(),
                )
                .unwrap();
            client.write_all(b"\n").unwrap();
            let shutdown =
                handle_shell_supervisor_stream(server, &root, &socket, "epoch+attach-stream")
                    .unwrap();
            assert!(!shutdown);
            let mut stream_body = String::new();
            client.read_to_string(&mut stream_body).unwrap();
            assert!(
                stream_body.contains(r#""method":"attach_stream""#),
                "{stream_body}"
            );
            assert!(
                stream_body.contains(r#""stream_method":"attach""#),
                "{stream_body}"
            );
            assert!(
                stream_body.contains(r#""terminal_raw_outputs""#),
                "{stream_body}"
            );
            assert!(stream_body.contains("native-pty-ready"), "{stream_body}");

            let (mut client, server) = std::os::unix::net::UnixStream::pair().unwrap();
            client
                .write_all(
                    format!(
                        r#"{{"method":"byte_stream","arguments":{{"task_id":"{task_id}","cursor":0,"limit_bytes":4000,"max_ms":500,"max_events":3,"poll_ms":25}}}}"#
                    )
                    .as_bytes(),
                )
                .unwrap();
            client.write_all(b"\n").unwrap();
            let shutdown =
                handle_shell_supervisor_stream(server, &root, &socket, "epoch+byte-stream")
                    .unwrap();
            assert!(!shutdown);
            let mut byte_stream_body = String::new();
            client.read_to_string(&mut byte_stream_body).unwrap();
            assert!(
                byte_stream_body.contains(r#""method":"byte_stream""#),
                "{byte_stream_body}"
            );
            assert!(
                byte_stream_body.contains(r#""stream_method":"pty_byte_stream""#),
                "{byte_stream_body}"
            );
            assert!(
                byte_stream_body.contains(r#""byte_outputs""#),
                "{byte_stream_body}"
            );
            assert!(
                byte_stream_body.contains(r#""bytes_base64""#),
                "{byte_stream_body}"
            );
            assert!(
                byte_stream_body.contains("native-pty-ready"),
                "{byte_stream_body}"
            );
        }

        let _ = crate::tools::exec_shell::ExecShellCancelTool.execute(
            ToolInput::new()
                .with_arg("cwd", root.display().to_string())
                .with_arg("task_id", task_id),
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn shell_supervisor_protocol_native_pty_resize_records_event() {
        if !native_supervisor_pty_supported() {
            return;
        }
        let root = temp_root("shell-supervisor-native-resize");
        let state_dir = root.join(".dscode/shell-supervisor");
        std::fs::create_dir_all(&state_dir).unwrap();
        let socket = state_dir.join("supervisor.sock");
        let command = if cfg!(windows) {
            "ping -n 6 127.0.0.1"
        } else {
            "tail -f /dev/null"
        };
        let start = parse_shell_supervisor_request(&format!(
            r#"{{"method":"start","arguments":{{"command":"{}","tty":true,"tty_rows":24,"tty_cols":80}}}}"#,
            json_escape(command)
        ))
        .unwrap();
        let response =
            shell_supervisor_protocol_response_for_request(&start, &root, &socket, "epoch+native");
        let object = json_as_object(&response).unwrap();
        assert_eq!(
            json_as_string(object.get("status").unwrap()),
            Some("ok"),
            "{response:?}"
        );
        let task_id = json_as_string(object.get("task_id").unwrap())
            .unwrap()
            .to_string();

        let resize = parse_shell_supervisor_request(&format!(
            r#"{{"method":"resize","arguments":{{"task_id":"{task_id}","tty_rows":32,"tty_cols":100}}}}"#
        ))
        .unwrap();
        let response =
            shell_supervisor_protocol_response_for_request(&resize, &root, &socket, "epoch+resize");
        let object = json_as_object(&response).unwrap();
        let resize_summary = json_as_string(object.get("resize_summary").unwrap()).unwrap();
        let expected_resize = if cfg!(windows) {
            "meta.live_resize=windows_conpty"
        } else {
            "meta.live_resize=native_tiocswinsz"
        };
        assert!(resize_summary.contains(expected_resize));
        assert!(resize_summary.contains("pty_backend: native-supervisor"));
        assert!(resize_summary.contains("tty_rows: 32"));
        assert!(resize_summary.contains("tty_cols: 100"));

        let replay = parse_shell_supervisor_request(&format!(
            r#"{{"method":"replay","arguments":{{"task_id":"{task_id}","stream":"terminal","cursor":0,"limit_bytes":4000}}}}"#
        ))
        .unwrap();
        let response =
            shell_supervisor_protocol_response_for_request(&replay, &root, &socket, "epoch+replay");
        let object = json_as_object(&response).unwrap();
        let replay_summary = json_as_string(object.get("replay_summary").unwrap()).unwrap();
        assert!(replay_summary.contains("[2 resize"));
        assert!(replay_summary.contains("rows=32 cols=100"));

        let _ = crate::tools::exec_shell::ExecShellCancelTool.execute(
            ToolInput::new()
                .with_arg("cwd", root.display().to_string())
                .with_arg("task_id", task_id),
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn shell_supervisor_protocol_controls_durable_jobs() {
        let root = temp_root("shell-supervisor-control");
        let state_dir = root.join(".dscode/shell-supervisor");
        std::fs::create_dir_all(&state_dir).unwrap();
        let socket = state_dir.join("supervisor.sock");

        let start = parse_shell_supervisor_request(
            r#"{"method":"start","arguments":{"command":"echo supervisor-control","tty":false}}"#,
        )
        .unwrap();
        let response =
            shell_supervisor_protocol_response_for_request(&start, &root, &socket, "epoch+start");
        let object = json_as_object(&response).unwrap();
        let task_id = json_as_string(object.get("task_id").unwrap())
            .unwrap()
            .to_string();
        assert_eq!(json_as_string(object.get("status").unwrap()), Some("ok"));
        assert!(matches!(
            object.get("active_jobs"),
            Some(JsonValue::Number(value)) if value == "1"
        ));

        let wait = parse_shell_supervisor_request(&format!(
            r#"{{"method":"wait","arguments":{{"task_id":"{task_id}","timeout_ms":1000}}}}"#
        ))
        .unwrap();
        let response =
            shell_supervisor_protocol_response_for_request(&wait, &root, &socket, "epoch+wait");
        let object = json_as_object(&response).unwrap();
        let wait_summary = json_as_string(object.get("wait_summary").unwrap()).unwrap();
        assert_eq!(json_as_string(object.get("status").unwrap()), Some("ok"));
        assert!(wait_summary.contains("status: completed"), "{wait_summary}");
        assert!(
            wait_summary.contains("supervisor-control"),
            "{wait_summary}"
        );
        assert!(matches!(
            object.get("active_jobs"),
            Some(JsonValue::Number(value)) if value == "0"
        ));

        let replay = parse_shell_supervisor_request(&format!(
            r#"{{"method":"replay","arguments":{{"task_id":"{task_id}","stream":"stdout"}}}}"#
        ))
        .unwrap();
        let response =
            shell_supervisor_protocol_response_for_request(&replay, &root, &socket, "epoch+replay");
        let object = json_as_object(&response).unwrap();
        let replay_summary = json_as_string(object.get("replay_summary").unwrap()).unwrap();
        assert_eq!(json_as_string(object.get("status").unwrap()), Some("ok"));
        assert!(
            replay_summary.contains("supervisor-control"),
            "{replay_summary}"
        );

        let attach = parse_shell_supervisor_request(&format!(
            r#"{{"method":"attach","arguments":{{"task_id":"{task_id}","cursor":0}}}}"#
        ))
        .unwrap();
        let response =
            shell_supervisor_protocol_response_for_request(&attach, &root, &socket, "epoch+attach");
        let object = json_as_object(&response).unwrap();
        let attach_summary = json_as_string(object.get("attach_summary").unwrap()).unwrap();
        assert_eq!(json_as_string(object.get("status").unwrap()), Some("ok"));
        assert!(
            attach_summary.contains("mode: terminal_attach_replay"),
            "{attach_summary}"
        );
        assert!(
            attach_summary.contains("supervisor-control"),
            "{attach_summary}"
        );

        let stdin_start = parse_shell_supervisor_request(
            r#"{"method":"start","arguments":{"command":"cat -","tty":false}}"#,
        )
        .unwrap();
        let response = shell_supervisor_protocol_response_for_request(
            &stdin_start,
            &root,
            &socket,
            "epoch+stdin-start",
        );
        let object = json_as_object(&response).unwrap();
        let stdin_task_id = json_as_string(object.get("task_id").unwrap())
            .unwrap()
            .to_string();

        let stdin = parse_shell_supervisor_request(&format!(
            r#"{{"method":"stdin","arguments":{{"task_id":"{stdin_task_id}","input":"hello supervisor stdin\n","close_stdin":true,"timeout_ms":1000}}}}"#
        ))
        .unwrap();
        let response =
            shell_supervisor_protocol_response_for_request(&stdin, &root, &socket, "epoch+stdin");
        let object = json_as_object(&response).unwrap();
        let stdin_summary = json_as_string(object.get("stdin_summary").unwrap()).unwrap();
        assert_eq!(json_as_string(object.get("status").unwrap()), Some("ok"));
        assert!(
            stdin_summary.contains("status: completed"),
            "{stdin_summary}"
        );
        assert!(
            stdin_summary.contains("hello supervisor stdin"),
            "{stdin_summary}"
        );

        let cancel_start = parse_shell_supervisor_request(
            r#"{"method":"start","arguments":{"command":"tail -f /dev/null","tty":false}}"#,
        )
        .unwrap();
        let response = shell_supervisor_protocol_response_for_request(
            &cancel_start,
            &root,
            &socket,
            "epoch+cancel-start",
        );
        let object = json_as_object(&response).unwrap();
        let cancel_task_id = json_as_string(object.get("task_id").unwrap())
            .unwrap()
            .to_string();
        assert!(matches!(
            object.get("active_jobs"),
            Some(JsonValue::Number(value)) if value == "1"
        ));

        let cancel = parse_shell_supervisor_request(&format!(
            r#"{{"method":"cancel","arguments":{{"task_id":"{cancel_task_id}"}}}}"#
        ))
        .unwrap();
        let response =
            shell_supervisor_protocol_response_for_request(&cancel, &root, &socket, "epoch+cancel");
        let object = json_as_object(&response).unwrap();
        let cancel_summary = json_as_string(object.get("cancel_summary").unwrap()).unwrap();
        assert_eq!(json_as_string(object.get("status").unwrap()), Some("ok"));
        assert!(
            cancel_summary.contains("Canceled background shell job"),
            "{cancel_summary}"
        );
        assert!(matches!(
            object.get("active_jobs"),
            Some(JsonValue::Number(value)) if value == "0"
        ));

        let resize = parse_shell_supervisor_request(
            r#"{"method":"resize","arguments":{"tty_rows":40,"tty_cols":120}}"#,
        )
        .unwrap();
        let response =
            shell_supervisor_protocol_response_for_request(&resize, &root, &socket, "epoch+resize");
        let object = json_as_object(&response).unwrap();
        assert_eq!(
            json_as_string(object.get("method").unwrap()),
            Some("resize")
        );
        assert_eq!(json_as_string(object.get("status").unwrap()), Some("error"));
        assert!(json_as_string(object.get("error").unwrap())
            .unwrap()
            .contains("requires task_id"));

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn shell_supervisor_protocol_show_includes_job_inventory() {
        let root = temp_root("shell-supervisor-show");
        let task_id = "shell-one";
        let job_dir = root.join(".dscode/shell-jobs").join(task_id);
        std::fs::create_dir_all(&job_dir).unwrap();
        std::fs::write(job_dir.join("stdout.log"), "durable\n").unwrap();
        let manifest = JsonValue::Object(BTreeMap::from([
            (
                "kind".to_string(),
                JsonValue::String("deepseek.exec_shell.job.v1".to_string()),
            ),
            ("id".to_string(), JsonValue::String(task_id.to_string())),
            (
                "command".to_string(),
                JsonValue::String("echo durable".to_string()),
            ),
            (
                "cwd".to_string(),
                JsonValue::String(root.display().to_string()),
            ),
            ("tty".to_string(), JsonValue::Bool(false)),
            ("pty_backend".to_string(), JsonValue::Null),
            ("attachable".to_string(), JsonValue::Bool(false)),
            ("resizable".to_string(), JsonValue::Bool(false)),
            ("supervisor_pid".to_string(), JsonValue::Null),
            ("supervisor_socket".to_string(), JsonValue::Null),
            ("supervisor_epoch".to_string(), JsonValue::Null),
            ("terminal_event_log".to_string(), JsonValue::Null),
            ("terminal_event_seq".to_string(), JsonValue::Null),
            ("control_token_hash".to_string(), JsonValue::Null),
            ("tty_rows".to_string(), JsonValue::Null),
            ("tty_cols".to_string(), JsonValue::Null),
            (
                "status".to_string(),
                JsonValue::String("exited".to_string()),
            ),
            ("exit_code".to_string(), JsonValue::Number("0".to_string())),
            ("pid".to_string(), JsonValue::Number("0".to_string())),
            ("owner_pid".to_string(), JsonValue::Null),
            ("process_group".to_string(), JsonValue::Null),
            ("stdin_path".to_string(), JsonValue::Null),
            ("stdin_keeper_pid".to_string(), JsonValue::Null),
            ("stdin_closed".to_string(), JsonValue::Bool(true)),
            (
                "started_at".to_string(),
                JsonValue::String("epoch+1".to_string()),
            ),
            (
                "updated_at".to_string(),
                JsonValue::String("epoch+2".to_string()),
            ),
            (
                "stdout_total_bytes".to_string(),
                JsonValue::Number("8".to_string()),
            ),
            (
                "stderr_total_bytes".to_string(),
                JsonValue::Number("0".to_string()),
            ),
        ]));
        std::fs::write(
            job_dir.join("manifest.json"),
            json_value_to_string(&manifest),
        )
        .unwrap();

        let response = shell_supervisor_protocol_response(
            "show",
            &root,
            &root.join(".dscode/shell-supervisor/supervisor.sock"),
            "epoch+3",
        );
        let object = json_as_object(&response).unwrap();
        let inventory = json_as_string(object.get("job_inventory").unwrap()).unwrap();

        assert_eq!(json_as_string(object.get("status").unwrap()), Some("ok"));
        assert_eq!(json_as_string(object.get("method").unwrap()), Some("show"));
        assert!(inventory.contains("Background shell jobs"), "{inventory}");
        assert!(inventory.contains(task_id), "{inventory}");
        assert!(inventory.contains("echo durable"), "{inventory}");
        assert!(inventory.contains("stdout=8"), "{inventory}");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn shell_supervisor_protocol_status_counts_active_durable_jobs() {
        let root = temp_root("shell-supervisor-status-active");
        let running_dir = root.join(".dscode/shell-jobs").join("shell-running");
        let exited_dir = root.join(".dscode/shell-jobs").join("shell-exited");
        std::fs::create_dir_all(&running_dir).unwrap();
        std::fs::create_dir_all(&exited_dir).unwrap();
        for (dir, id, status, pid) in [
            (
                &running_dir,
                "shell-running",
                "running",
                std::process::id().to_string(),
            ),
            (&exited_dir, "shell-exited", "exited", "0".to_string()),
        ] {
            let manifest = JsonValue::Object(BTreeMap::from([
                ("id".to_string(), JsonValue::String(id.to_string())),
                (
                    "command".to_string(),
                    JsonValue::String(format!("echo {id}")),
                ),
                (
                    "cwd".to_string(),
                    JsonValue::String(root.display().to_string()),
                ),
                ("status".to_string(), JsonValue::String(status.to_string())),
                ("pid".to_string(), JsonValue::Number(pid)),
                (
                    "started_at".to_string(),
                    JsonValue::String("epoch+1".to_string()),
                ),
                (
                    "updated_at".to_string(),
                    JsonValue::String("epoch+2".to_string()),
                ),
            ]));
            std::fs::write(dir.join("manifest.json"), json_value_to_string(&manifest)).unwrap();
        }

        let response = shell_supervisor_protocol_response(
            "status",
            &root,
            &root.join(".dscode/shell-supervisor/supervisor.sock"),
            "epoch+3",
        );
        let object = json_as_object(&response).unwrap();

        assert_eq!(json_as_string(object.get("status").unwrap()), Some("ok"));
        assert_eq!(
            json_as_string(object.get("method").unwrap()),
            Some("status")
        );
        assert!(matches!(
            object.get("active_jobs"),
            Some(JsonValue::Number(value)) if value == "1"
        ));
        assert!(!object.contains_key("active_jobs_error"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn shell_supervisor_protocol_refreshes_manifest_job_count() {
        let root = temp_root("shell-supervisor-manifest-refresh");
        let state_dir = root.join(".dscode/shell-supervisor");
        let job_dir = root.join(".dscode/shell-jobs").join("shell-running");
        std::fs::create_dir_all(&state_dir).unwrap();
        std::fs::create_dir_all(&job_dir).unwrap();
        std::fs::write(
            state_dir.join("manifest.json"),
            r#"{"kind":"deepseek.exec_shell.supervisor.v1","supervisor_pid":0,"supervisor_socket":"old.sock","supervisor_epoch":"epoch+old","protocol":"newline-json-v1","methods":["health","status","show","start","wait","replay","attach","attach_stream","byte_stream","pty_fd","stdin","resize","cancel","shutdown"],"unsupported_methods":[],"active_jobs":0,"started_at":"epoch+old","updated_at":"epoch+old","control_token_hash":"sha256:do-not-print"}"#,
        )
        .unwrap();
        let manifest = JsonValue::Object(BTreeMap::from([
            (
                "id".to_string(),
                JsonValue::String("shell-running".to_string()),
            ),
            (
                "command".to_string(),
                JsonValue::String("sleep 60".to_string()),
            ),
            (
                "cwd".to_string(),
                JsonValue::String(root.display().to_string()),
            ),
            (
                "status".to_string(),
                JsonValue::String("running".to_string()),
            ),
            (
                "pid".to_string(),
                JsonValue::Number(std::process::id().to_string()),
            ),
            (
                "started_at".to_string(),
                JsonValue::String("epoch+1".to_string()),
            ),
            (
                "updated_at".to_string(),
                JsonValue::String("epoch+2".to_string()),
            ),
        ]));
        std::fs::write(
            job_dir.join("manifest.json"),
            json_value_to_string(&manifest),
        )
        .unwrap();

        let socket = state_dir.join("supervisor.sock");
        let response = shell_supervisor_protocol_response("status", &root, &socket, "epoch+fresh");
        let object = json_as_object(&response).unwrap();
        let refreshed = std::fs::read_to_string(state_dir.join("manifest.json")).unwrap();

        assert_eq!(json_as_string(object.get("status").unwrap()), Some("ok"));
        assert!(!object.contains_key("manifest_refresh_error"));
        assert!(refreshed.contains(r#""active_jobs":1"#), "{refreshed}");
        assert!(
            refreshed.contains(r#""supervisor_socket":"#) && refreshed.contains("supervisor.sock"),
            "{refreshed}"
        );
        assert!(refreshed.contains(r#""supervisor_epoch":"epoch+fresh""#));
        assert!(refreshed.contains(r#""updated_at":"epoch+"#));
        assert!(refreshed.contains(r#""control_token_hash":null"#));
        assert!(!refreshed.contains("do-not-print"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    #[cfg(unix)]
    fn shell_supervisor_manifest_writes_protocol_without_control_secret() {
        let root = temp_root("shell-supervisor-manifest");
        let state_dir = root.join(".dscode/shell-supervisor");
        std::fs::create_dir_all(&state_dir).unwrap();
        let socket = state_dir.join("supervisor.sock");

        write_shell_supervisor_manifest(&root, &socket, "epoch+1").unwrap();

        let manifest = std::fs::read_to_string(state_dir.join("manifest.json")).unwrap();
        assert!(manifest.contains(r#""kind":"deepseek.exec_shell.supervisor.v1""#));
        assert!(manifest.contains(r#""protocol":"newline-json-v1""#));
        assert!(manifest.contains(r#""methods":["health","status","show","start","wait","replay","attach","attach_stream","byte_stream","pty_fd","stdin","resize","cancel","shutdown"]"#));
        assert!(manifest.contains(r#""unsupported_methods":[]"#));
        assert!(manifest.contains(r#""control_token_hash":null"#));
        assert!(!manifest.contains("control_token\":\""));
    }

    #[test]
    #[cfg(unix)]
    fn shell_supervisor_stream_handles_status_and_invalid_request() {
        let (shutdown, response) = shell_supervisor_stream_roundtrip(r#"{"method":"status"}"#);
        assert!(!shutdown);
        assert!(response.contains(r#""method":"status""#));
        assert!(response.contains(r#""status":"ok""#));

        let (shutdown, response) = shell_supervisor_stream_roundtrip("[]");
        assert!(!shutdown);
        assert!(response.contains(r#""method":"invalid_request""#));
        assert!(response.contains(r#""status":"error""#));
    }

    #[test]
    fn service_templates_render_runtime_and_agent_supervisors() {
        let config = ServiceTemplateConfig {
            kind: AgentsServiceKind::All,
            out: None,
            bin: "/usr/local/bin/deepseek".to_string(),
            workdir: "/work/repo".to_string(),
            addr: "127.0.0.1:9876".to_string(),
            interval_ms: 750,
            budget: Some(6),
        };

        let templates = service_templates(&config);
        let paths = templates
            .iter()
            .map(|template| template.path)
            .collect::<Vec<_>>();
        assert_eq!(
            paths,
            vec![
                "systemd/deepseek-runtime.service",
                "systemd/deepseek-agents.service",
                "systemd/deepseek-diagnostics.service",
                "systemd/deepseek-shell-supervisor.service",
                "launchd/com.deepseek.runtime.plist",
                "launchd/com.deepseek.agents.plist",
                "launchd/com.deepseek.diagnostics.plist",
                "launchd/com.deepseek.shell-supervisor.plist",
            ]
        );
        assert!(templates[0]
            .body
            .contains("serve --http --addr 127.0.0.1:9876"));
        assert!(templates[1]
            .body
            .contains("agents daemon --interval-ms 750 --budget 6 --json"));
        assert!(templates[1].body.contains("queued live RLM turn per tick"));
        assert!(templates[2]
            .body
            .contains("diagnostics --watch --changed --interval-ms 750 --json"));
        assert!(templates[3].body.contains("agents shell-supervisor --json"));
        assert!(templates[3]
            .body
            .contains("including native-supervisor PTY jobs where supported"));
        assert!(!templates[3]
            .body
            .contains("native PTY sessions are not implemented yet"));
        assert!(templates[4]
            .body
            .contains("<string>com.deepseek.runtime</string>"));
        assert!(templates[5]
            .body
            .contains("<string>com.deepseek.agents</string>"));
        assert!(templates[5].body.contains("queued live RLM turn per tick"));
        assert!(templates[6]
            .body
            .contains("<string>com.deepseek.diagnostics</string>"));
        assert!(templates[6].body.contains("<string>--json</string>"));
        assert!(templates[7]
            .body
            .contains("<string>com.deepseek.shell-supervisor</string>"));
        assert!(templates[7]
            .body
            .contains("<string>shell-supervisor</string>"));
        assert!(templates[7]
            .body
            .contains("including native-supervisor PTY jobs where supported"));
        assert!(!templates[7]
            .body
            .contains("native PTY sessions are not implemented yet"));

        let commands = templates
            .iter()
            .map(|template| {
                (
                    template.path,
                    service_template_command_vector(template).unwrap(),
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(
            commands[0].1.argv,
            vec![
                "/usr/local/bin/deepseek",
                "serve",
                "--http",
                "--addr",
                "127.0.0.1:9876"
            ]
        );
        assert_eq!(
            commands[1].1.argv,
            vec![
                "/usr/local/bin/deepseek",
                "agents",
                "daemon",
                "--interval-ms",
                "750",
                "--budget",
                "6",
                "--json"
            ]
        );
        assert_eq!(
            commands[2].1.argv,
            vec![
                "/usr/local/bin/deepseek",
                "diagnostics",
                "--watch",
                "--changed",
                "--interval-ms",
                "750",
                "--json"
            ]
        );
        assert_eq!(
            commands[3].1.argv,
            vec![
                "/usr/local/bin/deepseek",
                "agents",
                "shell-supervisor",
                "--json"
            ]
        );
        assert_eq!(commands[4].1.argv, commands[0].1.argv);
        assert_eq!(commands[5].1.argv, commands[1].1.argv);
        assert_eq!(commands[6].1.argv, commands[2].1.argv);
        assert_eq!(commands[7].1.argv, commands[3].1.argv);
    }

    #[test]
    fn service_template_command_vectors_handle_quoted_paths() {
        let config = ServiceTemplateConfig {
            kind: AgentsServiceKind::All,
            out: None,
            bin: "/tmp/DeepSeek Bin/deepseek".to_string(),
            workdir: "/tmp/DeepSeek Work/repo".to_string(),
            addr: "127.0.0.1:9876".to_string(),
            interval_ms: 750,
            budget: None,
        };

        let templates = service_templates(&config);
        for template in &templates {
            let command = service_template_command_vector(template).unwrap();
            assert_eq!(command.workdir, "/tmp/DeepSeek Work/repo");
            assert_eq!(command.argv[0], "/tmp/DeepSeek Bin/deepseek");
        }
        let report = build_service_doctor_report(config);
        assert!(
            report.checks.iter().any(|check| {
                check.name == "template_command_vectors" && check.status == ServiceDoctorStatus::Ok
            }),
            "{:?}",
            report.checks
        );
    }

    #[test]
    fn render_agent_services_writes_lifecycle_guide() {
        let out = temp_root("service-lifecycle").join("services");
        render_agent_services(AgentsServiceArgs {
            kind: AgentsServiceKind::All,
            out: Some(out.display().to_string()),
            bin: Some("/usr/local/bin/deepseek".to_string()),
            workdir: Some("/work/repo".to_string()),
            addr: "127.0.0.1:9876".to_string(),
            interval_ms: 750,
            budget: Some(6),
        })
        .unwrap();

        assert!(out.join("systemd/deepseek-runtime.service").is_file());
        assert!(out.join("launchd/com.deepseek.runtime.plist").is_file());
        let guide = std::fs::read_to_string(out.join("SERVICES.md")).unwrap();
        assert!(guide.contains("DeepSeekCode Service Lifecycle"));
        assert!(guide.contains("systemctl --user enable --now"));
        assert!(guide.contains("launchctl load -w"));
        assert!(guide.contains("journalctl --user"));
        assert!(guide.contains("launchctl kickstart -k"));
        assert!(guide.contains("curl -fsS http://127.0.0.1:9876/v1/health"));
        assert!(guide.contains("/usr/local/bin/deepseek agents shell status --json"));
    }

    #[test]
    fn service_doctor_reports_generated_service_health() {
        let workdir = temp_root("service-doctor-workdir");
        let out = workdir.join("services");
        let bin = std::env::current_exe().unwrap();
        render_agent_services(AgentsServiceArgs {
            kind: AgentsServiceKind::Systemd,
            out: Some(out.display().to_string()),
            bin: Some(bin.display().to_string()),
            workdir: Some(workdir.display().to_string()),
            addr: "127.0.0.1:9876".to_string(),
            interval_ms: 750,
            budget: Some(6),
        })
        .unwrap();

        let report = build_service_doctor_report(ServiceTemplateConfig {
            kind: AgentsServiceKind::Systemd,
            out: Some(out.clone()),
            bin: bin.display().to_string(),
            workdir: workdir.display().to_string(),
            addr: "127.0.0.1:9876".to_string(),
            interval_ms: 750,
            budget: Some(6),
        });

        assert_eq!(report.blocker_count(), 0, "{:?}", report.checks);
        assert!(report
            .checks
            .iter()
            .any(|check| check.name == "shell_supervisor_service"
                && check.status == ServiceDoctorStatus::Ok));
        assert!(report
            .checks
            .iter()
            .any(|check| check.name == "template_command_vectors"
                && check.status == ServiceDoctorStatus::Ok));
        let json = render_service_doctor_json(&report);
        assert!(json.contains("\"kind\":\"deepseek.agents.service_doctor.v1\""));
        assert!(json.contains("\"service_kind\":\"systemd\""));
        assert!(json.contains("\"installed\":false"));
        assert!(json.contains("\"blockers\":0"));
    }

    #[test]
    fn service_doctor_parses_systemd_installed_status() {
        let ready = parse_systemd_installed_service_status(
            "deepseek-runtime.service",
            "LoadState=loaded\nActiveState=active\nSubState=running\nUnitFileState=enabled\nFragmentPath=/home/me/.config/systemd/user/deepseek-runtime.service\n",
        )
        .unwrap();
        assert!(ready.is_ready(), "{ready:?}");
        assert!(ready.summary().contains("active_state=active"));

        let missing = parse_systemd_installed_service_status(
            "deepseek-runtime.service",
            "LoadState=not-found\nActiveState=inactive\nSubState=dead\nUnitFileState=\nFragmentPath=\n",
        )
        .unwrap();
        assert!(!missing.is_ready(), "{missing:?}");
        assert!(missing.summary().contains("load_state=not-found"));
    }

    #[test]
    fn service_doctor_parses_launchd_installed_status() {
        let ready = parse_launchd_installed_service_status(
            "com.deepseek.runtime",
            "state = running\npid = 123\nlast exit code = 0\npath = /Users/me/Library/LaunchAgents/com.deepseek.runtime.plist\n",
        )
        .unwrap();
        assert!(ready.is_ready(), "{ready:?}");
        assert!(ready.summary().contains("state=running"));

        let failed = parse_launchd_installed_service_status(
            "com.deepseek.runtime",
            "state = waiting\npid = 0\nLastExitStatus = 1\npath = /Users/me/Library/LaunchAgents/com.deepseek.runtime.plist\n",
        )
        .unwrap();
        assert!(!failed.is_ready(), "{failed:?}");
        assert!(failed.summary().contains("last_exit_status=1"));
    }

    #[test]
    fn service_doctor_detects_stale_generated_template() {
        let workdir = temp_root("service-doctor-stale");
        let out = workdir.join("services");
        let bin = std::env::current_exe().unwrap();
        render_agent_services(AgentsServiceArgs {
            kind: AgentsServiceKind::Systemd,
            out: Some(out.display().to_string()),
            bin: Some(bin.display().to_string()),
            workdir: Some(workdir.display().to_string()),
            addr: "127.0.0.1:9876".to_string(),
            interval_ms: 750,
            budget: None,
        })
        .unwrap();
        std::fs::write(out.join("systemd/deepseek-runtime.service"), "stale").unwrap();

        let report = build_service_doctor_report(ServiceTemplateConfig {
            kind: AgentsServiceKind::Systemd,
            out: Some(out.clone()),
            bin: bin.display().to_string(),
            workdir: workdir.display().to_string(),
            addr: "127.0.0.1:9876".to_string(),
            interval_ms: 750,
            budget: None,
        });

        assert!(report.blocker_count() >= 1, "{:?}", report.checks);
        assert!(report.checks.iter().any(|check| {
            check.status == ServiceDoctorStatus::Blocker
                && check.message.contains("stale or differs")
        }));
        let text = render_service_doctor_text(&report);
        assert!(text.contains("[blocker] output"));
        assert!(text.contains("summary:"));
    }

    #[test]
    fn service_smoke_resolves_ephemeral_loopback_addr() {
        let addr = resolve_service_smoke_addr("127.0.0.1:0").unwrap();
        let parsed = addr.parse::<std::net::SocketAddr>().unwrap();
        assert_eq!(parsed.ip().to_string(), "127.0.0.1");
        assert_ne!(parsed.port(), 0);
    }

    #[test]
    fn service_smoke_json_reports_blockers_and_warnings() {
        let mut report = ServiceSmokeReport {
            kind: AgentsServiceKind::Systemd,
            installed: false,
            binary: "/missing/deepseek".to_string(),
            workdir: PathBuf::from("/work/repo"),
            requested_addr: "127.0.0.1:0".to_string(),
            resolved_addr: "127.0.0.1:4567".to_string(),
            addr_error: None,
            timeout_ms: 2500,
            checks: Vec::new(),
        };
        push_service_doctor_check(
            &mut report.checks,
            ServiceDoctorStatus::Blocker,
            "runtime",
            "health failed",
        );
        push_service_doctor_check(
            &mut report.checks,
            ServiceDoctorStatus::Warn,
            "shell_supervisor",
            "unsupported platform",
        );

        let json = render_service_smoke_json(&report);
        assert!(json.contains("\"kind\":\"deepseek.agents.service_smoke.v1\""));
        assert!(json.contains("\"service_kind\":\"systemd\""));
        assert!(json.contains("\"installed\":false"));
        assert!(json.contains("\"blockers\":1"));
        assert!(json.contains("\"warnings\":1"));
        assert!(json.contains("\"resolved_addr\":\"127.0.0.1:4567\""));
    }

    #[test]
    fn service_smoke_installed_requires_concrete_addr() {
        let report = build_service_smoke_report(AgentsServiceSmokeArgs {
            kind: AgentsServiceKind::Systemd,
            bin: Some("/missing/deepseek".to_string()),
            workdir: Some("/work/repo".to_string()),
            addr: "127.0.0.1:0".to_string(),
            timeout_ms: 2500,
            installed: true,
            json: true,
        });

        assert!(report.installed);
        assert_eq!(report.resolved_addr, "127.0.0.1:0");
        assert!(report
            .addr_error
            .as_deref()
            .is_some_and(|error| error.contains("requires a concrete --addr port")));
        let json = render_service_smoke_json(&report);
        assert!(json.contains("\"installed\":true"));
    }

    #[cfg(unix)]
    #[test]
    fn service_smoke_shell_supervisor_control_smoke_runs_start_wait_attach() {
        use std::io::{BufRead as _, Read as _, Write as _};

        let root = std::env::temp_dir().join(format!(
            "dsc-smk-ctl-{}-{}",
            std::process::id(),
            current_epoch_seconds()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let socket = root.join("supervisor.sock");
        let listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
        let tty = cfg!(all(unix, target_os = "linux"));
        let expected_methods: Vec<&'static str> = if tty {
            vec![
                "start",
                "wait",
                "attach",
                "replay",
                "start",
                "stdin",
                "resize",
                "attach",
                "replay",
                "cancel",
                "start",
                "byte_stream",
                "start",
                "byte_stream",
            ]
        } else {
            vec!["start", "wait", "attach", "replay"]
        };
        let handle = std::thread::spawn(move || {
            for (step, expected) in expected_methods.into_iter().enumerate() {
                let (mut stream, _) = listener.accept().unwrap();
                let mut reader = std::io::BufReader::new(stream.try_clone().unwrap());
                let mut request = String::new();
                reader.read_line(&mut request).unwrap();
                assert!(request.contains(&format!(r#""method":"{expected}""#)));
                match (step, expected) {
                    (0, "start") => {
                        assert!(request.contains(if tty {
                            r#""tty":true"#
                        } else {
                            r#""tty":false"#
                        }));
                        let backend = if tty { "native-supervisor" } else { "none" };
                        stream
                            .write_all(
                                format!(
                                    r#"{{"status":"ok","method":"start","task_id":"task-smoke","job_pty_backend":"{backend}"}}"#
                                )
                                .as_bytes(),
                            )
                            .unwrap();
                    }
                    (_, "wait") => {
                        assert!(request.contains(r#""task_id":"task-smoke""#));
                        stream
                            .write_all(
                                br#"{"status":"ok","method":"wait","wait_summary":"status: exited"}"#,
                            )
                            .unwrap();
                    }
                    (2, "attach") => {
                        assert!(request.contains(r#""task_id":"task-smoke""#));
                        stream
                            .write_all(
                                br#"{"status":"ok","method":"attach","attach_summary":"deepseek-shell-supervisor-smoke"}"#,
                            )
                            .unwrap();
                    }
                    (3, "replay") => {
                        assert!(request.contains(r#""task_id":"task-smoke""#));
                        stream
                            .write_all(
                                br#"{"status":"ok","method":"replay","replay_summary":"deepseek-shell-supervisor-smoke"}"#,
                            )
                            .unwrap();
                    }
                    (4, "start") => {
                        assert!(request.contains(r#""tty":true"#));
                        assert!(request.contains("cat -"));
                        stream
                            .write_all(
                                br#"{"status":"ok","method":"start","task_id":"task-pty","job_pty_backend":"native-supervisor"}"#,
                            )
                            .unwrap();
                    }
                    (_, "stdin") => {
                        assert!(request.contains(r#""task_id":"task-pty""#));
                        assert!(request.contains("deepseek-pty-control"));
                        stream
                            .write_all(
                                br#"{"status":"ok","method":"stdin","stdin_summary":"status: running deepseek-pty-control"}"#,
                            )
                            .unwrap();
                    }
                    (_, "resize") => {
                        assert!(request.contains(r#""task_id":"task-pty""#));
                        assert!(request.contains(r#""tty_rows":31"#));
                        assert!(request.contains(r#""tty_cols":99"#));
                        stream
                            .write_all(
                                br#"{"status":"ok","method":"resize","resize_summary":"meta.live_resize=native_tiocswinsz"}"#,
                            )
                            .unwrap();
                    }
                    (7, "attach") => {
                        assert!(request.contains(r#""task_id":"task-pty""#));
                        stream
                            .write_all(
                                br#"{"status":"ok","method":"attach","attach_summary":"deepseek-pty-control"}"#,
                            )
                            .unwrap();
                    }
                    (8, "replay") => {
                        assert!(request.contains(r#""task_id":"task-pty""#));
                        stream
                            .write_all(
                                br#"{"status":"ok","method":"replay","replay_summary":"deepseek-pty-control\n[2 resize] rows=31 cols=99"}"#,
                            )
                            .unwrap();
                    }
                    (_, "cancel") => {
                        assert!(request.contains(r#""task_id":"task-pty""#));
                        stream
                            .write_all(
                                br#"{"status":"ok","method":"cancel","cancel_summary":"Canceled background shell job: task-pty\nstatus: killed"}"#,
                            )
                            .unwrap();
                    }
                    (10, "start") => {
                        assert!(request.contains(r#""tty":true"#));
                        assert!(request.contains("head -n 1"));
                        stream
                            .write_all(
                                br#"{"status":"ok","method":"start","task_id":"task-byte","job_pty_backend":"native-supervisor"}"#,
                            )
                            .unwrap();
                    }
                    (11, "byte_stream") => {
                        assert!(request.contains(r#""task_id":"task-byte""#));
                        let mut resize_frame = String::new();
                        reader.read_line(&mut resize_frame).unwrap();
                        assert!(resize_frame.contains(r#""type":"resize""#));
                        assert!(resize_frame.contains(r#""rows":36"#));
                        assert!(resize_frame.contains(r#""cols":104"#));
                        let mut stdin_frame = String::new();
                        reader.read_line(&mut stdin_frame).unwrap();
                        assert!(stdin_frame.contains(r#""type":"stdin""#));
                        assert!(stdin_frame.contains("frame-probe"));
                        stream
                            .write_all(
                                br#"{"status":"ok","method":"byte_stream","stream_method":"pty_byte_stream","stream_done":true,"control_frames":[{"type":"resize","resize_summary":"meta.live_resize=native_tiocswinsz"},{"type":"stdin","stdin_summary":"status: running"}],"byte_outputs":[{"bytes_base64":"ZnJhbWUtcHJvYmUK"}],"attach_summary":"frame-probe"}"#,
                            )
                            .unwrap();
                    }
                    (12, "start") => {
                        assert!(request.contains(r#""tty":true"#));
                        assert!(request.contains("head -n 1"));
                        stream
                            .write_all(
                                br#"{"status":"ok","method":"start","task_id":"task-raw","job_pty_backend":"native-supervisor"}"#,
                            )
                            .unwrap();
                    }
                    (13, "byte_stream") => {
                        assert!(request.contains(r#""task_id":"task-raw""#));
                        assert!(request.contains(r#""raw_proxy":true"#));
                        let mut raw_input = String::new();
                        reader.read_to_string(&mut raw_input).unwrap();
                        assert!(raw_input.contains("raw-probe"));
                        stream.write_all(b"raw-probe\r\nraw-probe\r\n").unwrap();
                    }
                    _ => unreachable!(),
                }
                stream.write_all(b"\n").unwrap();
            }
        });

        let summary = shell_supervisor_control_smoke(&socket, 2500, None).unwrap();
        handle.join().unwrap();

        assert!(summary.contains("task-smoke"));
        assert!(summary.contains(&format!("tty={tty}")));
        assert!(summary.contains("start/wait/attach/replay"));
        if tty {
            assert!(summary.contains("PTY stdin/resize/replay"));
            assert!(summary.contains("PTY cancel"));
        }

        let _ = std::fs::remove_dir_all(&root);
    }

    #[cfg(unix)]
    #[test]
    fn service_smoke_blocks_too_long_shell_supervisor_socket_path() {
        let long_workdir =
            PathBuf::from(format!("/tmp/{}", "dsc-service-smoke-long-path-".repeat(5)));
        let mut report = ServiceSmokeReport {
            kind: AgentsServiceKind::Systemd,
            installed: false,
            binary: "/missing/deepseek".to_string(),
            workdir: long_workdir,
            requested_addr: "127.0.0.1:0".to_string(),
            resolved_addr: "127.0.0.1:4567".to_string(),
            addr_error: None,
            timeout_ms: 2500,
            checks: Vec::new(),
        };

        service_smoke_check_shell_supervisor(&mut report);

        assert!(report.checks.iter().any(|check| {
            check.status == ServiceDoctorStatus::Blocker
                && check.name == "shell_supervisor"
                && check.message.contains("socket path is too long")
        }));
    }

    #[cfg(unix)]
    #[test]
    fn service_smoke_blocks_existing_shell_supervisor_socket() {
        use std::io::{BufRead as _, Write as _};

        let root = std::env::temp_dir().join(format!(
            "dsc-smk-{}-{}",
            std::process::id(),
            current_epoch_seconds()
        ));
        let _ = std::fs::remove_dir_all(&root);
        let state_dir = root.join(".dscode/shell-supervisor");
        std::fs::create_dir_all(&state_dir).unwrap();
        let socket = state_dir.join("supervisor.sock");
        let listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut reader = std::io::BufReader::new(stream.try_clone().unwrap());
            let mut request = String::new();
            reader.read_line(&mut request).unwrap();
            assert!(request.contains("\"method\":\"health\""));
            stream
                .write_all(b"{\"status\":\"ok\",\"method\":\"health\"}\n")
                .unwrap();
        });
        let mut report = ServiceSmokeReport {
            kind: AgentsServiceKind::Systemd,
            installed: false,
            binary: "/missing/deepseek".to_string(),
            workdir: root,
            requested_addr: "127.0.0.1:0".to_string(),
            resolved_addr: "127.0.0.1:4567".to_string(),
            addr_error: None,
            timeout_ms: 2500,
            checks: Vec::new(),
        };

        service_smoke_check_shell_supervisor(&mut report);
        handle.join().unwrap();

        assert!(report.checks.iter().any(|check| {
            check.status == ServiceDoctorStatus::Blocker
                && check.name == "shell_supervisor"
                && check.message.contains("already active")
        }));
        let _ = std::fs::remove_dir_all(&report.workdir);
    }

    #[test]
    fn runtime_task_approval_resolver_waits_for_durable_response() {
        let store = RuntimeStore::new(temp_root("runtime-approval"));
        let session = store
            .create_session("Runtime approval".to_string(), ".".to_string())
            .unwrap();
        let thread = store
            .create_thread_for_session(
                &session.id,
                "Approval work".to_string(),
                ".".to_string(),
                "deepseek-v4-flash".to_string(),
                "agent".to_string(),
            )
            .unwrap();
        let responder_store = store.clone();
        let responder_thread_id = thread.id.clone();
        let responder = std::thread::spawn(move || {
            for _ in 0..50 {
                let events = responder_store
                    .read_events(&responder_thread_id, 0)
                    .expect("events should read");
                if let Some(request) = events
                    .iter()
                    .find(|event| event.kind == "permission_request")
                {
                    responder_store
                        .append_permission_response(
                            &responder_thread_id,
                            None,
                            request.id.clone(),
                            "approved".to_string(),
                        )
                        .expect("response should append");
                    return;
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            panic!("permission request was not written");
        });

        let mut resolver = RuntimeTaskApprovalResolver {
            store: store.clone(),
            thread_id: thread.id.clone(),
            poll_interval: Duration::from_millis(5),
            max_polls: Some(200),
        };
        let decision = resolver
            .resolve(&AgentApprovalRequest {
                tool_name: "apply_patch".to_string(),
                input: std::collections::BTreeMap::new(),
                kind: "write".to_string(),
                target: "src/lib.rs".to_string(),
            })
            .unwrap();
        responder.join().unwrap();

        assert_eq!(decision, AgentApprovalDecision::Approved);
        let events = store.read_events(&thread.id, 0).unwrap();
        assert!(events
            .iter()
            .any(|event| event.kind == "permission_request"));
        assert!(events
            .iter()
            .any(|event| event.kind == "permission_response"));
    }

    #[test]
    fn runtime_task_user_input_resolver_waits_for_durable_response() {
        let store = RuntimeStore::new(temp_root("runtime-user-input"));
        let session = store
            .create_session("Runtime user input".to_string(), ".".to_string())
            .unwrap();
        let thread = store
            .create_thread_for_session(
                &session.id,
                "Clarify work".to_string(),
                ".".to_string(),
                "deepseek-v4-flash".to_string(),
                "agent".to_string(),
            )
            .unwrap();
        let responder_store = store.clone();
        let responder_thread_id = thread.id.clone();
        let responder = std::thread::spawn(move || {
            for _ in 0..50 {
                let events = responder_store
                    .read_events(&responder_thread_id, 0)
                    .expect("events should read");
                if let Some(request) = events
                    .iter()
                    .find(|event| event.kind == "user_input_request")
                {
                    responder_store
                        .append_user_input_response(
                            &responder_thread_id,
                            None,
                            request.id.clone(),
                            std::collections::BTreeMap::from([(
                                "mode".to_string(),
                                "Plan".to_string(),
                            )]),
                        )
                        .expect("response should append");
                    return;
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            panic!("user input request was not written");
        });

        let mut resolver = RuntimeTaskUserInputResolver {
            store: store.clone(),
            thread_id: thread.id.clone(),
            poll_interval: Duration::from_millis(5),
            max_polls: Some(200),
        };
        let questions = r#"[{"header":"Mode","id":"mode","question":"Which mode?","options":[{"label":"Plan","description":"Plan first."},{"label":"Apply","description":"Implement directly."}]}]"#;
        let response = resolver
            .resolve(&AgentUserInputRequest {
                input: std::collections::BTreeMap::from([(
                    "questions".to_string(),
                    questions.to_string(),
                )]),
            })
            .unwrap();
        responder.join().unwrap();

        assert_eq!(
            response.answers.get("mode").map(String::as_str),
            Some("Plan")
        );
        let events = store.read_events(&thread.id, 0).unwrap();
        assert!(events
            .iter()
            .any(|event| event.kind == "user_input_request"));
        assert!(events
            .iter()
            .any(|event| event.kind == "user_input_response"));
    }

    #[test]
    fn record_runtime_task_result_writes_turns_items_usage_and_task_status() {
        let store = RuntimeStore::new(temp_root("runtime-task-result"));
        let session = store
            .create_session("Runtime runner".to_string(), ".".to_string())
            .unwrap();
        let thread = store
            .create_thread_for_session(
                &session.id,
                "Queued work".to_string(),
                ".".to_string(),
                "deepseek-v4-flash".to_string(),
                "agent".to_string(),
            )
            .unwrap();
        let task = store
            .create_task(
                Some(&session.id),
                Some(&thread.id),
                None,
                "automation".to_string(),
                "running".to_string(),
                "run queued work".to_string(),
            )
            .unwrap();
        let mut usage = crate::model::protocol::TokenUsage::new(8, 2);
        usage.model = Some("deepseek-v4-flash".to_string());
        let result = RunResult {
            final_message: "done".to_string(),
            tool_events: vec![ToolEvent {
                tool_name: "read_file".to_string(),
                input: std::collections::BTreeMap::from([(
                    "path".to_string(),
                    "README.md".to_string(),
                )]),
                output: "ok".to_string(),
                status: ObservationStatus::Ok,
            }],
            usage,
            prompt_layers: vec![crate::core::prompt_layers::PromptLayerSnapshot {
                step: 1,
                layers: vec![crate::core::prompt_layers::PromptLayerRecord {
                    name: "system_static".to_string(),
                    text_sha256: "abc123".to_string(),
                    bytes: 12,
                    estimated_tokens: 3,
                    cache_stable: true,
                }],
                total_bytes: 12,
                estimated_tokens: 3,
            }],
            model_routes: Vec::new(),
            tool_repairs: Vec::new(),
        };

        let assistant_turn_id =
            record_runtime_task_result(&store, &task, &thread, &result).unwrap();

        assert!(assistant_turn_id.starts_with("turn-"));
        let updated = store.load_task(&task.id).unwrap();
        assert_eq!(updated.status, "completed");
        assert_eq!(updated.summary, "done");
        let turns = store.list_turns(&thread.id).unwrap();
        assert_eq!(turns.len(), 2);
        assert_eq!(turns[0].role, "user");
        assert_eq!(turns[1].role, "assistant");
        let items = store.list_items(&thread.id, None).unwrap();
        assert_eq!(items.len(), 3);
        assert_eq!(items[2].item_type, "tool_result");
        assert!(items[2].content.contains("tool: read_file"));
        let usage = store.list_usage(Some(&thread.id), 10).unwrap();
        assert_eq!(usage.len(), 1);
        assert_eq!(usage[0].source, "runtime_runner");
        assert_eq!(usage[0].total_tokens, 10);
        let events = store.read_events(&thread.id, 0).unwrap();
        assert!(events.iter().any(|event| event.kind == "task_updated"));
        assert!(events.iter().any(|event| event.kind == "usage_recorded"));
        assert!(events
            .iter()
            .any(|event| event.kind == "prompt_layers_recorded"));
    }

    #[test]
    fn record_runtime_task_failure_writes_failed_item_and_status() {
        let store = RuntimeStore::new(temp_root("runtime-task-failure"));
        let thread = store
            .create_thread(
                "Queued work".to_string(),
                ".".to_string(),
                "deepseek-v4-flash".to_string(),
                "agent".to_string(),
            )
            .unwrap();
        let task = store
            .create_task(
                None,
                Some(&thread.id),
                None,
                "automation".to_string(),
                "running".to_string(),
                "run queued work".to_string(),
            )
            .unwrap();

        record_runtime_task_failure(&store, &task, &thread, "boom").unwrap();

        let updated = store.load_task(&task.id).unwrap();
        assert_eq!(updated.status, "failed");
        assert!(updated.summary.contains("boom"));
        let items = store.list_items(&thread.id, None).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].status, "failed");
        assert!(items[0].content.contains("runtime task failed"));
    }

    #[test]
    fn run_runtime_task_executes_pending_thread_task_end_to_end() {
        let root = temp_root("runtime-task-run");
        let workspace = root.join("workspace");
        std::fs::create_dir_all(&workspace).unwrap();
        std::fs::write(workspace.join("README.md"), "hello runtime task\n").unwrap();
        let config_dir = root.join(".dscode");
        let mut config = AppConfig::default();
        config.workspace.config_dir = config_dir.display().to_string();
        config.workspace.session_dir = config_dir.join("sessions").display().to_string();
        config.model.api_key_env = "DSCODE_TEST_NO_KEY".to_string();
        let store = RuntimeStore::new(config_dir.join("runtime"));
        let session = store
            .create_session(
                "Runtime runner".to_string(),
                workspace.display().to_string(),
            )
            .unwrap();
        let thread = store
            .create_thread_for_session(
                &session.id,
                "Queued work".to_string(),
                workspace.display().to_string(),
                "deepseek-coder".to_string(),
                "agent".to_string(),
            )
            .unwrap();
        let task = store
            .create_task(
                Some(&session.id),
                Some(&thread.id),
                None,
                "automation".to_string(),
                "pending".to_string(),
                "inspect repository layout".to_string(),
            )
            .unwrap();
        let original_dir = {
            let _cwd_lock = crate::util::cwd::lock_cwd().unwrap();
            std::env::current_dir().unwrap()
        };

        run_runtime_task(config, &task.id, Some(1), true).unwrap();

        let restored_dir = {
            let _cwd_lock = crate::util::cwd::lock_cwd().unwrap();
            std::env::current_dir().unwrap()
        };
        assert_eq!(restored_dir, original_dir);
        let updated = store.load_task(&task.id).unwrap();
        assert_eq!(updated.status, "completed");
        let turns = store.list_turns(&thread.id).unwrap();
        assert_eq!(turns.len(), 2);
        let items = store.list_items(&thread.id, None).unwrap();
        assert!(items
            .iter()
            .any(|item| item.item_type == "tool_result" && item.content.contains("README.md")));
        let events = store.read_events(&thread.id, 0).unwrap();
        assert!(events.iter().any(|event| event.kind == "task_claimed"));
        assert!(events.iter().any(|event| event.kind == "task_updated"));
    }

    #[test]
    fn daemon_schedule_parser_accepts_common_interval_shapes() {
        assert_eq!(parse_schedule_interval_seconds("every:5m"), Some(300));
        assert_eq!(parse_schedule_interval_seconds("@every 2h"), Some(7_200));
        assert_eq!(parse_schedule_interval_seconds("interval:30s"), Some(30));
        assert_eq!(parse_schedule_interval_seconds("manual"), None);
        assert_eq!(parse_schedule_interval_seconds("once"), None);
        assert_eq!(parse_schedule_interval_seconds("0s"), None);
    }

    #[test]
    fn runtime_daemon_tick_executes_pending_thread_task() {
        let root = temp_root("runtime-daemon-task");
        let workspace = root.join("workspace");
        std::fs::create_dir_all(&workspace).unwrap();
        std::fs::write(workspace.join("README.md"), "hello runtime daemon\n").unwrap();
        let config_dir = root.join(".dscode");
        let mut config = AppConfig::default();
        config.workspace.config_dir = config_dir.display().to_string();
        config.workspace.session_dir = config_dir.join("sessions").display().to_string();
        config.model.api_key_env = "DSCODE_TEST_NO_KEY".to_string();
        let store = RuntimeStore::new(config_dir.join("runtime"));
        let thread = store
            .create_thread(
                "Queued work".to_string(),
                workspace.display().to_string(),
                "deepseek-coder".to_string(),
                "agent".to_string(),
            )
            .unwrap();
        let task = store
            .create_task(
                None,
                Some(&thread.id),
                None,
                "manual".to_string(),
                "pending".to_string(),
                "inspect repository layout".to_string(),
            )
            .unwrap();

        let tick = run_runtime_daemon_tick(&config, &store, Some(1), true).unwrap();

        assert_eq!(tick.executed_tasks, 1);
        assert_eq!(tick.triggered_automations, 0);
        let updated = store.load_task(&task.id).unwrap();
        assert_eq!(updated.status, "completed");
    }

    #[test]
    fn runtime_daemon_tick_routes_live_rlm_turns_through_rlm_worker() {
        let root = temp_root("runtime-daemon-rlm-live");
        let workspace = root.join("workspace");
        std::fs::create_dir_all(&workspace).unwrap();
        std::fs::write(workspace.join("README.md"), "hello live rlm daemon\n").unwrap();
        let config_dir = root.join(".dscode");
        let mut config = AppConfig::default();
        config.workspace.config_dir = config_dir.display().to_string();
        config.workspace.session_dir = config_dir.join("sessions").display().to_string();
        config.model.api_key_env = "DSCODE_TEST_NO_KEY".to_string();
        let store = RuntimeStore::new(config_dir.join("runtime"));
        let queued = crate::tools::rlm::RlmTool {
            tool_name: "rlm_process",
            config: config.clone(),
            parent_depth: 0,
        }
        .execute(
            ToolInput::new()
                .with_arg("task", "summarize live daemon payload")
                .with_arg("content", "live daemon payload")
                .with_arg("session_id", "daemon.rlm")
                .with_arg("live", "true")
                .with_arg("cwd", workspace.display().to_string()),
        )
        .unwrap();
        let turn_id = meta_value(&queued.summary, "meta.rlm_turn_id").unwrap();

        let tick = run_runtime_daemon_tick(&config, &store, Some(1), true).unwrap();

        assert_eq!(tick.executed_rlm_turns, 1);
        assert_eq!(tick.executed_tasks, 0);
        let updated = store.load_task(&turn_id).unwrap();
        assert_eq!(updated.status, "completed");
        let events = crate::tools::rlm::RlmLiveEventsTool {
            config: config.clone(),
        }
        .execute(ToolInput::new().with_arg("session_id", "daemon.rlm"))
        .unwrap();
        assert!(events.summary.contains(r#""kind":"turn_started""#));
        assert!(events.summary.contains(r#""kind":"turn_completed""#));
    }

    #[test]
    fn runtime_daemon_tick_recovers_stale_live_rlm_owner_before_running_queue() {
        let root = temp_root("runtime-daemon-rlm-stale-owner");
        let workspace = root.join("workspace");
        std::fs::create_dir_all(&workspace).unwrap();
        std::fs::write(workspace.join("README.md"), "hello stale rlm owner\n").unwrap();
        let config_dir = root.join(".dscode");
        let mut config = AppConfig::default();
        config.workspace.config_dir = config_dir.display().to_string();
        config.workspace.session_dir = config_dir.join("sessions").display().to_string();
        config.model.api_key_env = "DSCODE_TEST_NO_KEY".to_string();
        let store = RuntimeStore::new(config_dir.join("runtime"));
        let queued = crate::tools::rlm::RlmTool {
            tool_name: "rlm_process",
            config: config.clone(),
            parent_depth: 0,
        }
        .execute(
            ToolInput::new()
                .with_arg("task", "recover stale daemon owner payload")
                .with_arg("content", "stale owner payload")
                .with_arg("session_id", "daemon.stale")
                .with_arg("live", "true")
                .with_arg("cwd", workspace.display().to_string()),
        )
        .unwrap();
        let turn_id = meta_value(&queued.summary, "meta.rlm_turn_id").unwrap();
        let thread_id = meta_value(&queued.summary, "meta.rlm_runtime_thread_id").unwrap();
        store
            .claim_task(&turn_id, "test-stale-owner".to_string())
            .unwrap();
        let manifest_dir = config_dir.join("rlm-daemon").join("daemon.stale");
        std::fs::create_dir_all(&manifest_dir).unwrap();
        std::fs::write(
            manifest_dir.join("manifest.json"),
            format!(
                r#"{{"session_id":"daemon.stale","status":"running","daemon_pid":{},"daemon_epoch":"epoch+stale","runtime_thread_id":"{}","runtime_session_id":null,"active_turn_id":"{}","queued_turns":0,"model":"deepseek-coder","workspace":"{}","created_at":"epoch+1","updated_at":"epoch+2","last_error":null}}"#,
                i32::MAX as u64 + 1,
                thread_id,
                turn_id,
                workspace.display()
            ),
        )
        .unwrap();

        let tick = run_runtime_daemon_tick(&config, &store, Some(1), true).unwrap();

        assert_eq!(tick.recovered_rlm_turns, 1);
        assert_eq!(tick.executed_rlm_turns, 1);
        assert_eq!(tick.failed_rlm_recoveries, 0);
        let updated = store.load_task(&turn_id).unwrap();
        assert_eq!(updated.status, "completed");
        let events = crate::tools::rlm::RlmLiveEventsTool {
            config: config.clone(),
        }
        .execute(ToolInput::new().with_arg("session_id", "daemon.stale"))
        .unwrap();
        assert!(events.summary.contains(r#""kind":"turn_recovered""#));
        assert!(events.summary.contains(r#""kind":"turn_completed""#));
    }

    struct RecordingSummaryClient {
        request: std::cell::RefCell<Option<ModelRequest>>,
        message: String,
    }

    impl ModelClient for RecordingSummaryClient {
        fn respond(
            &self,
            input: ModelRequest,
            _events: &mut dyn crate::ui::stream::StreamEvents,
        ) -> AppResult<(
            crate::model::protocol::ModelResponse,
            Option<crate::model::protocol::TokenUsage>,
        )> {
            *self.request.borrow_mut() = Some(input);
            Ok((
                crate::model::protocol::ModelResponse {
                    message: self.message.clone(),
                    action: ModelAction::Finish,
                },
                None,
            ))
        }
    }

    #[test]
    fn model_compaction_summary_request_captures_prior_context() {
        let store = RuntimeStore::new(temp_root("model-compact-request"));
        let thread = store
            .create_thread(
                "Model compact".to_string(),
                ".".to_string(),
                "deepseek-v4-flash".to_string(),
                "agent".to_string(),
            )
            .unwrap();
        store
            .append_turn(
                &thread.id,
                "user".to_string(),
                "key decision: keep the Rust CLI local-first".to_string(),
            )
            .unwrap();
        store
            .append_turn(
                &thread.id,
                "assistant".to_string(),
                "implemented runtime state and wrote docs".to_string(),
            )
            .unwrap();
        store
            .append_turn(&thread.id, "user".to_string(), "tail request".to_string())
            .unwrap();
        store
            .append_turn(
                &thread.id,
                "assistant".to_string(),
                "tail answer".to_string(),
            )
            .unwrap();
        let turns = store.list_turns(&thread.id).unwrap();
        let client = RecordingSummaryClient {
            request: std::cell::RefCell::new(None),
            message: "Model summary: local-first Rust CLI, runtime docs done.".to_string(),
        };

        let summary = model_compaction_summary_with_client(&client, &thread, &turns, 2).unwrap();

        assert_eq!(
            summary,
            "Model summary: local-first Rust CLI, runtime docs done."
        );
        let request = client.request.borrow();
        let request = request.as_ref().expect("expected model request");
        assert_eq!(request.profile_name, "runtime-compaction");
        assert!(request.available_tools.is_empty());
        assert!(!request.planning_mode);
        assert!(request.system_prompt.contains("automatic compaction"));
        assert!(request.task.contains("Thread title: Model compact"));
        assert!(request
            .task
            .contains("key decision: keep the Rust CLI local-first"));
        assert!(request.task.contains("Tail turns preserved verbatim: 2"));
        assert!(request.task.contains("The live tail begins at turn #3"));
    }

    #[test]
    fn runtime_daemon_tick_compacts_threads_after_usage_warning() {
        let root = temp_root("runtime-daemon-compact");
        let config_dir = root.join(".dscode");
        let mut config = AppConfig::default();
        config.workspace.config_dir = config_dir.display().to_string();
        config.model.api_key_env = "DSCODE_TEST_NO_KEY".to_string();
        let store = RuntimeStore::new(config_dir.join("runtime"));
        let thread = store
            .create_thread(
                "Long context".to_string(),
                ".".to_string(),
                "deepseek-v4-flash".to_string(),
                "agent".to_string(),
            )
            .unwrap();
        let mut latest_turn_id = String::new();
        for index in 1..=10 {
            latest_turn_id = store
                .append_turn(&thread.id, "assistant".to_string(), format!("turn {index}"))
                .unwrap()
                .id;
        }
        store
            .append_usage_with_cache(
                &thread.id,
                Some(&latest_turn_id),
                "deepseek-v4-flash".to_string(),
                "test".to_string(),
                850_000,
                25,
                200_000,
                650_000,
            )
            .unwrap();

        let first_tick = run_runtime_daemon_tick(&config, &store, None, false).unwrap();
        let second_tick = run_runtime_daemon_tick(&config, &store, None, false).unwrap();

        assert_eq!(first_tick.compacted_threads, 1);
        assert_eq!(second_tick.compacted_threads, 0);
        let events = store.read_events(&thread.id, 0).unwrap();
        assert_eq!(
            events
                .iter()
                .filter(|event| event.kind == "thread_compacted")
                .count(),
            1
        );
    }

    #[test]
    fn runtime_daemon_tick_respects_configured_compaction_threshold() {
        let root = temp_root("runtime-daemon-compact-threshold");
        let config_dir = root.join(".dscode");
        let mut config = AppConfig::default();
        config.workspace.config_dir = config_dir.display().to_string();
        config.model.api_key_env = "DSCODE_TEST_NO_KEY".to_string();
        config.runtime.daemon_compaction_threshold_tokens = 900_000;
        let store = RuntimeStore::new(config_dir.join("runtime"));
        let thread = store
            .create_thread(
                "Long context below configured threshold".to_string(),
                ".".to_string(),
                "deepseek-v4-flash".to_string(),
                "agent".to_string(),
            )
            .unwrap();
        let mut latest_turn_id = String::new();
        for index in 1..=10 {
            latest_turn_id = store
                .append_turn(&thread.id, "assistant".to_string(), format!("turn {index}"))
                .unwrap()
                .id;
        }
        store
            .append_usage_with_cache(
                &thread.id,
                Some(&latest_turn_id),
                "deepseek-v4-flash".to_string(),
                "test".to_string(),
                850_000,
                25,
                200_000,
                650_000,
            )
            .unwrap();

        let tick = run_runtime_daemon_tick(&config, &store, None, false).unwrap();

        assert_eq!(tick.compacted_threads, 0);
        assert!(!store
            .read_events(&thread.id, 0)
            .unwrap()
            .iter()
            .any(|event| event.kind == "thread_compacted"));
    }

    #[test]
    fn runtime_daemon_compaction_uses_model_summary_provider() {
        let root = temp_root("runtime-daemon-model-compact");
        let config_dir = root.join(".dscode");
        let store = RuntimeStore::new(config_dir.join("runtime"));
        let thread = store
            .create_thread(
                "Long model context".to_string(),
                ".".to_string(),
                "deepseek-v4-flash".to_string(),
                "agent".to_string(),
            )
            .unwrap();
        let mut latest_turn_id = String::new();
        for index in 1..=10 {
            latest_turn_id = store
                .append_turn(&thread.id, "assistant".to_string(), format!("turn {index}"))
                .unwrap()
                .id;
        }
        store
            .append_usage_with_cache(
                &thread.id,
                Some(&latest_turn_id),
                "deepseek-v4-flash".to_string(),
                "test".to_string(),
                850_000,
                25,
                200_000,
                650_000,
            )
            .unwrap();
        let mut tick = RuntimeDaemonTick::default();
        let mut called = 0usize;

        run_runtime_daemon_compactions_with_summary_provider(
            &store,
            false,
            &mut tick,
            RuntimeDaemonCompactionSettings {
                threshold_tokens: 800_000,
                keep_tail_turns: 8,
            },
            |_store, _thread| {
                called += 1;
                Ok(Some("Generated model context summary".to_string()))
            },
        )
        .unwrap();

        assert_eq!(called, 1);
        assert_eq!(tick.compacted_threads, 1);
        let items = store.list_items(&thread.id, None).unwrap();
        assert!(items.iter().any(|item| {
            item.item_type == "summary" && item.content == "Generated model context summary"
        }));
        let events = store.read_events(&thread.id, 0).unwrap();
        let compaction_event = events
            .iter()
            .find(|event| event.kind == "thread_compacted")
            .expect("expected compaction event");
        let JsonValue::Object(payload) = &compaction_event.payload else {
            panic!("expected object payload");
        };
        assert_eq!(
            payload
                .get("summary_source")
                .and_then(crate::util::json::json_as_string),
            Some("model")
        );
    }

    #[test]
    fn runtime_daemon_tick_triggers_due_automation_and_runs_task() {
        let root = temp_root("runtime-daemon-automation");
        let workspace = root.join("workspace");
        std::fs::create_dir_all(&workspace).unwrap();
        std::fs::write(workspace.join("README.md"), "hello automation\n").unwrap();
        let config_dir = root.join(".dscode");
        let mut config = AppConfig::default();
        config.workspace.config_dir = config_dir.display().to_string();
        config.workspace.session_dir = config_dir.join("sessions").display().to_string();
        config.model.api_key_env = "DSCODE_TEST_NO_KEY".to_string();
        let store = RuntimeStore::new(config_dir.join("runtime"));
        let session = store
            .create_session(
                "Runtime daemon".to_string(),
                workspace.display().to_string(),
            )
            .unwrap();
        let thread = store
            .create_thread_for_session(
                &session.id,
                "Scheduled work".to_string(),
                workspace.display().to_string(),
                "deepseek-coder".to_string(),
                "agent".to_string(),
            )
            .unwrap();
        let due_at = format_epoch_seconds(current_epoch_seconds().saturating_sub(1));
        let automation = store
            .create_automation(
                Some(&session.id),
                Some(&thread.id),
                "Nightly check".to_string(),
                "active".to_string(),
                "every:60s".to_string(),
                "inspect repository layout".to_string(),
                None,
                Some(due_at),
            )
            .unwrap();

        let tick = run_runtime_daemon_tick(&config, &store, Some(1), true).unwrap();

        assert_eq!(tick.triggered_automations, 1);
        assert_eq!(tick.executed_tasks, 1);
        let tasks = store
            .list_tasks(Some(&session.id), Some(&thread.id), 10)
            .unwrap();
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].status, "completed");
        let updated_automation = store.load_automation(&automation.id).unwrap();
        assert!(updated_automation.last_run_at.is_some());
        assert!(updated_automation.next_run_at.is_some());
        assert_ne!(updated_automation.next_run_at.as_deref(), Some("epoch+0"));
        let events = store.read_events(&thread.id, 0).unwrap();
        assert!(events
            .iter()
            .any(|event| event.kind == "automation_triggered"));
        assert!(events
            .iter()
            .any(|event| event.kind == "automation_scheduled"));
    }

    fn temp_root(name: &str) -> std::path::PathBuf {
        let suffix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "deepseek-agents-{name}-{}-{suffix}",
            std::process::id()
        ))
    }

    #[cfg(unix)]
    fn shell_supervisor_stream_roundtrip(request: &str) -> (bool, String) {
        let (mut client, server) = std::os::unix::net::UnixStream::pair().unwrap();
        let handle = std::thread::spawn(move || {
            handle_shell_supervisor_stream(
                server,
                Path::new("/work/repo"),
                Path::new("/work/repo/.dscode/shell-supervisor/supervisor.sock"),
                "epoch+1",
            )
            .unwrap()
        });

        client.write_all(request.as_bytes()).unwrap();
        client.write_all(b"\n").unwrap();
        let mut response = String::new();
        let mut reader = BufReader::new(&mut client);
        reader.read_line(&mut response).unwrap();
        let shutdown = handle.join().unwrap();
        (shutdown, response)
    }

    fn meta_value(summary: &str, key: &str) -> Option<String> {
        summary
            .lines()
            .find_map(|line| line.strip_prefix(&format!("{key}=")))
            .map(str::trim)
            .map(str::to_string)
    }
}
