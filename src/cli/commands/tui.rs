use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use crate::cli::app::TuiArgs;
use crate::config::load::load_or_default;
use crate::config::types::AppConfig;
use crate::core::context::TaskContext;
use crate::core::loop_runtime::{
    AgentApprovalDecision, AgentApprovalRequest, AgentApprovalResolver, AgentCancelCheck,
    AgentLoop, AgentLoopOptions, RunResult, SharedAgentApprovalResolver, SharedAgentCancelCheck,
    ToolEvent,
};
use crate::core::rollback::RollbackStore;
use crate::core::runtime::{
    json_object, parse_automation_record, parse_item_record, parse_runtime_event,
    parse_session_record, parse_task_record, parse_thread_record, parse_usage_record, RuntimeEvent,
    RuntimeStore,
};
use crate::error::{app_error, AppResult};
use crate::tui::{
    render_once, run_interactive, run_interactive_with_refresh_actions_and_live, TuiAction, TuiApp,
    TuiApprovalRequest, TuiAutomationRecord, TuiItem, TuiLiveEvent, TuiSession, TuiTaskRecord,
    TuiThread, TuiUsageSummary,
};
use crate::ui::stream::StreamEvents;
use crate::util::json::{
    json_as_array, json_as_object, json_as_string, json_value_to_string, parse_json_value,
    JsonValue,
};
use crate::util::sse;

pub fn run(args: TuiArgs) -> AppResult<()> {
    if args.demo {
        let app = TuiApp::demo();
        if args.once {
            print!("{}", render_once(&app, 120, 36)?);
            return Ok(());
        }
        return run_interactive(app);
    }

    if let Some(runtime_url) = args.runtime_url.as_deref() {
        return run_http_runtime_tui(runtime_url, args.once);
    }

    let config = load_or_default()?;
    let runtime_root = PathBuf::from(&config.workspace.config_dir).join("runtime");
    let runtime_store = RuntimeStore::new(runtime_root);
    let app = app_from_store(&runtime_store)?;

    if args.once {
        print!("{}", render_once(&app, 120, 36)?);
        return Ok(());
    }

    let (live_tx, live_rx) = mpsc::channel();
    let _runtime_watcher = start_runtime_live_watcher(
        runtime_store.clone(),
        live_tx.clone(),
        Duration::from_millis(250),
    );
    let refresh_store = runtime_store.clone();
    let action_store = runtime_store.clone();
    run_interactive_with_refresh_actions_and_live(
        app,
        Duration::from_secs(1),
        move |app| refresh_app_from_store(&refresh_store, app),
        move |app, action| {
            handle_tui_action_with_live(
                &action_store,
                Some(&config),
                app,
                action,
                Some(live_tx.clone()),
            )
        },
        move |app| drain_tui_live_events(&live_rx, app),
    )
}

struct RuntimeSnapshot {
    sessions: Vec<TuiSession>,
    threads: Vec<TuiThread>,
    items: Vec<TuiItem>,
    tasks: Vec<TuiTaskRecord>,
    automations: Vec<TuiAutomationRecord>,
    usage_summaries: Vec<TuiUsageSummary>,
    approvals: Vec<TuiApprovalRequest>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RuntimeSnapshotSignature {
    sessions: Vec<(String, String, Option<String>, u64)>,
    threads: Vec<(String, String, String, Option<String>, u64)>,
    item_count: usize,
    tasks: Vec<(String, String, String)>,
    automations: Vec<(String, String, Option<String>, Option<String>)>,
    usage_summaries: Vec<(String, usize, u64, u64)>,
    approvals: Vec<(String, String)>,
}

struct RuntimeLiveWatcher {
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl Drop for RuntimeLiveWatcher {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

#[derive(Debug, Clone)]
struct RuntimeHttpClient {
    authority: String,
    host: String,
    port: u16,
    path_prefix: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RuntimeHttpSubscription {
    thread_id: String,
    since_seq: u64,
}

struct RuntimeHttpLiveWatcher {
    stop: Arc<AtomicBool>,
    handles: Vec<JoinHandle<()>>,
}

impl Drop for RuntimeHttpLiveWatcher {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        for handle in self.handles.drain(..) {
            let _ = handle.join();
        }
    }
}

impl RuntimeHttpClient {
    fn from_url(url: &str) -> AppResult<Self> {
        let trimmed = url.trim().trim_end_matches('/');
        let rest = trimmed
            .strip_prefix("http://")
            .ok_or_else(|| app_error("tui --runtime-url currently supports http:// URLs only"))?;
        let (authority, path_prefix) = match rest.split_once('/') {
            Some((authority, path)) => (authority, format!("/{path}")),
            None => (rest, String::new()),
        };
        if authority.is_empty() {
            return Err(app_error("tui --runtime-url is missing a host"));
        }
        let (host, port) = match authority.rsplit_once(':') {
            Some((host, port)) if !host.is_empty() => {
                let port = port.parse::<u16>().map_err(|_| {
                    app_error(format!("tui --runtime-url has invalid port `{port}`"))
                })?;
                (host.to_string(), port)
            }
            _ => (authority.to_string(), 80),
        };
        Ok(Self {
            authority: authority.to_string(),
            host,
            port,
            path_prefix,
        })
    }

    fn request_target(&self, path: &str) -> String {
        if self.path_prefix.is_empty() {
            path.to_string()
        } else {
            format!("{}{}", self.path_prefix, path)
        }
    }

    fn connect(&self) -> AppResult<TcpStream> {
        TcpStream::connect((self.host.as_str(), self.port)).map_err(|error| {
            app_error(format!(
                "failed to connect to HTTP runtime at {}: {error}",
                self.authority
            ))
        })
    }

    fn get_json(&self, path: &str) -> AppResult<JsonValue> {
        self.request_json("GET", path, None)
    }

    fn post_json(&self, path: &str, body: JsonValue) -> AppResult<JsonValue> {
        self.request_json("POST", path, Some(json_value_to_string(&body)))
    }

    fn request_json(&self, method: &str, path: &str, body: Option<String>) -> AppResult<JsonValue> {
        let mut stream = self.connect()?;
        let target = self.request_target(path);
        let body = body.unwrap_or_default();
        let content_headers = if body.is_empty() {
            String::new()
        } else {
            format!(
                "Content-Type: application/json; charset=utf-8\r\nContent-Length: {}\r\n",
                body.len()
            )
        };
        let request = format!(
            "{method} {target} HTTP/1.1\r\nHost: {}\r\nAccept: application/json\r\n{}Connection: close\r\n\r\n{}",
            self.authority, content_headers, body
        );
        stream.write_all(request.as_bytes())?;
        stream.flush()?;
        let mut raw = String::new();
        stream.read_to_string(&mut raw)?;
        let (status, response_body) = parse_http_response(&raw)?;
        if !(200..300).contains(&status) {
            return Err(app_error(format!(
                "HTTP runtime request {method} {path} failed with {status}: {}",
                response_body.trim()
            )));
        }
        parse_json_value(response_body)
    }

    fn open_sse(&self, path: &str) -> AppResult<BufReader<TcpStream>> {
        let mut stream = self.connect()?;
        let target = self.request_target(path);
        let request = format!(
            "GET {target} HTTP/1.1\r\nHost: {}\r\nAccept: text/event-stream\r\nConnection: close\r\n\r\n",
            self.authority
        );
        stream.write_all(request.as_bytes())?;
        stream.flush()?;
        let mut reader = BufReader::new(stream);
        let mut status_line = String::new();
        reader.read_line(&mut status_line)?;
        let status = parse_status_code(&status_line)?;
        let mut header = String::new();
        loop {
            header.clear();
            let read = reader.read_line(&mut header)?;
            if read == 0 || header == "\r\n" || header == "\n" {
                break;
            }
        }
        if !(200..300).contains(&status) {
            return Err(app_error(format!(
                "HTTP runtime SSE request {path} failed with {status}"
            )));
        }
        Ok(reader)
    }
}

fn run_http_runtime_tui(runtime_url: &str, once: bool) -> AppResult<()> {
    let client = RuntimeHttpClient::from_url(runtime_url)?;
    let snapshot = runtime_http_snapshot(&client)?;
    let subscriptions = runtime_http_subscriptions(&snapshot);
    let app = app_from_snapshot(snapshot);

    if once {
        print!("{}", render_once(&app, 120, 36)?);
        return Ok(());
    }

    let (live_tx, live_rx) = mpsc::channel();
    let _runtime_watcher =
        start_runtime_http_live_watcher(client.clone(), subscriptions, live_tx.clone(), 250);
    let refresh_client = client.clone();
    let action_client = client.clone();
    run_interactive_with_refresh_actions_and_live(
        app,
        Duration::from_secs(1),
        move |app| refresh_app_from_http(&refresh_client, app),
        move |app, action| handle_tui_http_action(&action_client, app, action),
        move |app| drain_tui_live_events(&live_rx, app),
    )
}

fn app_from_snapshot(snapshot: RuntimeSnapshot) -> TuiApp {
    TuiApp::with_runtime_usage_tasks_automations_and_approvals(
        snapshot.sessions,
        snapshot.threads,
        snapshot.items,
        snapshot.tasks,
        snapshot.automations,
        snapshot.usage_summaries,
        snapshot.approvals,
    )
}

fn refresh_app_from_http(client: &RuntimeHttpClient, app: &mut TuiApp) -> AppResult<()> {
    app.apply_live_event(snapshot_live_event(runtime_http_snapshot(client)?));
    Ok(())
}

fn runtime_http_subscriptions(snapshot: &RuntimeSnapshot) -> Vec<RuntimeHttpSubscription> {
    snapshot
        .threads
        .iter()
        .map(|thread| RuntimeHttpSubscription {
            thread_id: thread.id.clone(),
            since_seq: thread.event_seq,
        })
        .collect()
}

fn start_runtime_http_live_watcher(
    client: RuntimeHttpClient,
    subscriptions: Vec<RuntimeHttpSubscription>,
    tx: Sender<TuiLiveEvent>,
    follow_max_ms: u64,
) -> RuntimeHttpLiveWatcher {
    let stop = Arc::new(AtomicBool::new(false));
    let mut handles = Vec::new();
    for subscription in subscriptions {
        let worker_client = client.clone();
        let worker_tx = tx.clone();
        let worker_stop = stop.clone();
        let handle = thread::spawn(move || {
            follow_runtime_thread_events(
                worker_client,
                subscription,
                worker_tx,
                worker_stop,
                follow_max_ms,
            )
        });
        handles.push(handle);
    }
    RuntimeHttpLiveWatcher { stop, handles }
}

fn follow_runtime_thread_events(
    client: RuntimeHttpClient,
    mut subscription: RuntimeHttpSubscription,
    tx: Sender<TuiLiveEvent>,
    stop: Arc<AtomicBool>,
    follow_max_ms: u64,
) {
    while !stop.load(Ordering::Relaxed) {
        let path = format!(
            "/v1/threads/{}/events/stream?since_seq={}&follow=1&poll_ms=100&max_ms={}",
            subscription.thread_id, subscription.since_seq, follow_max_ms
        );
        match client.open_sse(&path) {
            Ok(mut reader) => loop {
                if stop.load(Ordering::Relaxed) {
                    break;
                }
                match sse::read_frame(&mut reader) {
                    Ok(Some(frame)) => {
                        if let Ok(event) = runtime_event_from_sse_frame(&frame) {
                            subscription.since_seq = event.seq;
                            match runtime_http_snapshot(&client) {
                                Ok(snapshot) => {
                                    if tx.send(snapshot_live_event(snapshot)).is_err() {
                                        return;
                                    }
                                }
                                Err(error) => {
                                    if tx
                                        .send(TuiLiveEvent::Status(format!(
                                            "runtime HTTP refresh failed: {error}"
                                        )))
                                        .is_err()
                                    {
                                        return;
                                    }
                                }
                            }
                        }
                    }
                    Ok(None) => break,
                    Err(error) => {
                        if tx
                            .send(TuiLiveEvent::Status(format!(
                                "runtime SSE read failed: {error}"
                            )))
                            .is_err()
                        {
                            return;
                        }
                        break;
                    }
                }
            },
            Err(error) => {
                if tx
                    .send(TuiLiveEvent::Status(format!(
                        "runtime SSE connect failed: {error}"
                    )))
                    .is_err()
                {
                    return;
                }
                thread::sleep(Duration::from_millis(follow_max_ms.min(1_000)));
            }
        }
    }
}

fn runtime_event_from_sse_frame(frame: &sse::SseFrame) -> AppResult<RuntimeEvent> {
    let value = parse_json_value(&frame.data)?;
    let root = json_object_value(value, "runtime event SSE frame")?;
    parse_runtime_event(&root)
}

fn runtime_http_snapshot(client: &RuntimeHttpClient) -> AppResult<RuntimeSnapshot> {
    let sessions_root = json_object_value(client.get_json("/v1/sessions?limit=50")?, "sessions")?;
    let session_records = parse_record_array(&sessions_root, "sessions", parse_session_record)?;
    let mut threads = Vec::new();
    let mut items = Vec::new();
    let mut tasks = Vec::new();
    let mut automations = Vec::new();
    let mut usage_summaries = Vec::new();
    let mut approvals = Vec::new();

    for session in &session_records {
        let session_root = json_object_value(
            client.get_json(&format!("/v1/sessions/{}", session.id))?,
            "session",
        )?;
        let session_threads = parse_record_array(&session_root, "threads", parse_thread_record)?;
        for thread in session_threads {
            let thread_id = thread.id.clone();
            let thread_root = json_object_value(
                client.get_json(&format!("/v1/threads/{thread_id}"))?,
                "thread",
            )?;
            let detail_thread = parse_record_object(&thread_root, "thread", parse_thread_record)?;
            items.extend(
                parse_record_array(&thread_root, "items", parse_item_record)?
                    .into_iter()
                    .map(TuiItem::from),
            );

            let task_root = json_object_value(
                client.get_json(&format!("/v1/threads/{thread_id}/tasks?limit=20"))?,
                "tasks",
            )?;
            tasks.extend(
                parse_record_array(&task_root, "tasks", parse_task_record)?
                    .into_iter()
                    .map(TuiTaskRecord::from),
            );

            let automation_root = json_object_value(
                client.get_json(&format!("/v1/threads/{thread_id}/automations?limit=20"))?,
                "automations",
            )?;
            automations.extend(
                parse_record_array(&automation_root, "automations", parse_automation_record)?
                    .into_iter()
                    .map(TuiAutomationRecord::from),
            );

            let usage_root = json_object_value(
                client.get_json(&format!("/v1/threads/{thread_id}/usage?limit=200"))?,
                "usage",
            )?;
            let usage = parse_record_array(&usage_root, "usage", parse_usage_record)?;
            if !usage.is_empty() {
                usage_summaries.push(TuiUsageSummary::from_usage_records(&thread_id, &usage));
            }

            let events_root = json_object_value(
                client.get_json(&format!("/v1/threads/{thread_id}/events?since_seq=0"))?,
                "events",
            )?;
            let events = parse_record_array(&events_root, "events", parse_runtime_event)?;
            let resolved_approval_ids = events
                .iter()
                .filter_map(TuiApprovalRequest::response_request_id)
                .collect::<BTreeSet<_>>();
            approvals.extend(events.iter().filter_map(|event| {
                let approval = TuiApprovalRequest::from_runtime_event(event)?;
                if resolved_approval_ids.contains(&approval.id) {
                    None
                } else {
                    Some(approval)
                }
            }));

            threads.push(TuiThread::from(detail_thread));
        }
    }

    Ok(RuntimeSnapshot {
        sessions: session_records.into_iter().map(TuiSession::from).collect(),
        threads,
        items,
        tasks,
        automations,
        usage_summaries,
        approvals,
    })
}

fn handle_tui_http_action(
    client: &RuntimeHttpClient,
    app: &mut TuiApp,
    action: TuiAction,
) -> AppResult<()> {
    match action {
        TuiAction::SubmitUserMessage { thread_id, content } => {
            client.post_json(
                &format!("/v1/threads/{thread_id}/turns"),
                json_object([
                    ("role", JsonValue::String("user".to_string())),
                    ("content", JsonValue::String(content)),
                ]),
            )?;
            app.set_status(format!(
                "submitted user message to remote runtime {thread_id}"
            ));
        }
        TuiAction::RespondApproval {
            thread_id,
            turn_id,
            request_id,
            decision,
        } => {
            let mut body = BTreeMap::new();
            body.insert(
                "type".to_string(),
                JsonValue::String("permission_response".to_string()),
            );
            body.insert(
                "request_id".to_string(),
                JsonValue::String(request_id.clone()),
            );
            body.insert("decision".to_string(), JsonValue::String(decision.clone()));
            if let Some(turn_id) = turn_id {
                body.insert("turn_id".to_string(), JsonValue::String(turn_id));
            }
            client.post_json(
                &format!("/v1/threads/{thread_id}/events"),
                JsonValue::Object(body),
            )?;
            app.set_status(format!(
                "recorded remote approval response: {request_id} {decision}"
            ));
        }
        TuiAction::CancelRun { thread_id, turn_id } => {
            let mut body = BTreeMap::new();
            body.insert(
                "type".to_string(),
                JsonValue::String("cancel_requested".to_string()),
            );
            body.insert(
                "reason".to_string(),
                JsonValue::String("user requested cancellation from TUI".to_string()),
            );
            if let Some(turn_id) = turn_id {
                body.insert("turn_id".to_string(), JsonValue::String(turn_id));
            }
            client.post_json(
                &format!("/v1/threads/{thread_id}/events"),
                JsonValue::Object(body),
            )?;
            app.set_status(format!("remote cancel event recorded for {thread_id}"));
        }
        TuiAction::CreateTask { thread_id, summary } => {
            client.post_json(
                &format!("/v1/threads/{thread_id}/tasks"),
                json_object([
                    ("kind", JsonValue::String("agent".to_string())),
                    ("status", JsonValue::String("pending".to_string())),
                    ("summary", JsonValue::String(summary)),
                ]),
            )?;
            app.set_status(format!("created remote pending task for {thread_id}"));
        }
        TuiAction::PauseTask { task_id } => {
            client.post_json(
                &format!("/v1/tasks/{task_id}/pause"),
                JsonValue::Object(BTreeMap::new()),
            )?;
            app.set_status(format!("paused remote task {task_id}"));
        }
        TuiAction::ResumeTask { task_id } => {
            client.post_json(
                &format!("/v1/tasks/{task_id}/resume"),
                JsonValue::Object(BTreeMap::new()),
            )?;
            app.set_status(format!("resumed remote task {task_id}"));
        }
        TuiAction::RunDiagnostics { changed, paths } => {
            run_remote_tui_diagnostics(client, app, changed, paths);
        }
        TuiAction::CreateRollbackSnapshot { .. }
        | TuiAction::ListRollbackSnapshots { .. }
        | TuiAction::ShowRollbackSnapshot { .. }
        | TuiAction::RevertTurn { .. } => {
            app.set_status("rollback commands require local file-backed TUI".to_string());
        }
        TuiAction::TriggerAutomation {
            automation_id,
            prompt_override,
        } => {
            let body = match prompt_override {
                Some(prompt) => json_object([("prompt", JsonValue::String(prompt))]),
                None => JsonValue::Object(BTreeMap::new()),
            };
            client.post_json(&format!("/v1/automations/{automation_id}/trigger"), body)?;
            app.set_status(format!("triggered remote automation {automation_id}"));
        }
        TuiAction::CompactThread {
            thread_id,
            keep_tail_turns,
        } => {
            client.post_json(
                &format!("/v1/threads/{thread_id}/compact"),
                json_object([(
                    "keep_tail_turns",
                    JsonValue::Number(keep_tail_turns.to_string()),
                )]),
            )?;
            app.set_status(format!("compacted remote thread {thread_id}"));
        }
    }
    Ok(())
}

fn parse_http_response(raw: &str) -> AppResult<(u16, &str)> {
    let Some((head, body)) = raw.split_once("\r\n\r\n") else {
        return Err(app_error("malformed HTTP runtime response"));
    };
    let status_line = head.lines().next().unwrap_or("");
    Ok((parse_status_code(status_line)?, body))
}

fn parse_status_code(status_line: &str) -> AppResult<u16> {
    status_line
        .split_whitespace()
        .nth(1)
        .and_then(|value| value.parse::<u16>().ok())
        .ok_or_else(|| app_error(format!("malformed HTTP status line `{status_line}`")))
}

fn json_object_value(value: JsonValue, context: &str) -> AppResult<BTreeMap<String, JsonValue>> {
    let JsonValue::Object(root) = value else {
        return Err(app_error(format!(
            "{context} response root must be an object"
        )));
    };
    Ok(root)
}

fn parse_record_object<T>(
    root: &BTreeMap<String, JsonValue>,
    key: &str,
    parse: fn(&BTreeMap<String, JsonValue>) -> AppResult<T>,
) -> AppResult<T> {
    let value = root
        .get(key)
        .ok_or_else(|| app_error(format!("HTTP runtime response missing `{key}`")))?;
    let object = json_as_object(value)
        .ok_or_else(|| app_error(format!("HTTP runtime response `{key}` must be an object")))?;
    parse(object)
}

fn parse_record_array<T>(
    root: &BTreeMap<String, JsonValue>,
    key: &str,
    parse: fn(&BTreeMap<String, JsonValue>) -> AppResult<T>,
) -> AppResult<Vec<T>> {
    let value = root
        .get(key)
        .ok_or_else(|| app_error(format!("HTTP runtime response missing `{key}`")))?;
    let array = json_as_array(value)
        .ok_or_else(|| app_error(format!("HTTP runtime response `{key}` must be an array")))?;
    array
        .iter()
        .map(|item| {
            let object = json_as_object(item).ok_or_else(|| {
                app_error(format!(
                    "HTTP runtime response `{key}` array item must be an object"
                ))
            })?;
            parse(object)
        })
        .collect()
}

fn runtime_snapshot(store: &RuntimeStore) -> AppResult<RuntimeSnapshot> {
    let session_records = store.list_sessions(50)?;
    let mut threads = Vec::new();
    let mut items = Vec::new();
    let mut tasks = Vec::new();
    let mut automations = Vec::new();
    let mut usage_summaries = Vec::new();
    let mut approvals = Vec::new();
    for session in &session_records {
        for thread in store.list_session_threads(&session.id, 50)? {
            let events = store.read_events(&thread.id, 0)?;
            let resolved_approval_ids = events
                .iter()
                .filter_map(TuiApprovalRequest::response_request_id)
                .collect::<BTreeSet<_>>();
            items.extend(
                store
                    .list_items(&thread.id, None)?
                    .into_iter()
                    .map(TuiItem::from),
            );
            tasks.extend(
                store
                    .list_tasks(Some(&session.id), Some(&thread.id), 20)?
                    .into_iter()
                    .map(TuiTaskRecord::from),
            );
            automations.extend(
                store
                    .list_automations(Some(&session.id), Some(&thread.id), 20)?
                    .into_iter()
                    .map(TuiAutomationRecord::from),
            );
            let usage = store.list_usage(Some(&thread.id), usize::MAX)?;
            if !usage.is_empty() {
                usage_summaries.push(TuiUsageSummary::from_usage_records(&thread.id, &usage));
            }
            approvals.extend(events.iter().filter_map(|event| {
                let approval = TuiApprovalRequest::from_runtime_event(event)?;
                if resolved_approval_ids.contains(&approval.id) {
                    None
                } else {
                    Some(approval)
                }
            }));
            threads.push(TuiThread::from(thread));
        }
    }
    let sessions = session_records
        .into_iter()
        .map(TuiSession::from)
        .collect::<Vec<_>>();
    Ok(RuntimeSnapshot {
        sessions,
        threads,
        items,
        tasks,
        automations,
        usage_summaries,
        approvals,
    })
}

fn runtime_snapshot_signature(snapshot: &RuntimeSnapshot) -> RuntimeSnapshotSignature {
    let mut sessions = snapshot
        .sessions
        .iter()
        .map(|session| {
            (
                session.id.clone(),
                session.status.clone(),
                session.active_thread_id.clone(),
                session.thread_count,
            )
        })
        .collect::<Vec<_>>();
    sessions.sort();

    let mut threads = snapshot
        .threads
        .iter()
        .map(|thread| {
            (
                thread.id.clone(),
                thread.status.clone(),
                thread.mode.clone(),
                thread.latest_turn_id.clone(),
                thread.event_seq,
            )
        })
        .collect::<Vec<_>>();
    threads.sort();

    let mut tasks = snapshot
        .tasks
        .iter()
        .map(|task| {
            (
                task.id.clone(),
                task.status.clone(),
                task.updated_at.clone(),
            )
        })
        .collect::<Vec<_>>();
    tasks.sort();

    let mut automations = snapshot
        .automations
        .iter()
        .map(|automation| {
            (
                automation.id.clone(),
                automation.status.clone(),
                automation.last_run_at.clone(),
                automation.next_run_at.clone(),
            )
        })
        .collect::<Vec<_>>();
    automations.sort();

    let mut usage_summaries = snapshot
        .usage_summaries
        .iter()
        .map(|usage| {
            (
                usage.thread_id.clone(),
                usage.record_count,
                usage.total_tokens,
                usage.latest_total_tokens,
            )
        })
        .collect::<Vec<_>>();
    usage_summaries.sort();

    let mut approvals = snapshot
        .approvals
        .iter()
        .map(|approval| (approval.id.clone(), approval.status.clone()))
        .collect::<Vec<_>>();
    approvals.sort();

    RuntimeSnapshotSignature {
        sessions,
        threads,
        item_count: snapshot.items.len(),
        tasks,
        automations,
        usage_summaries,
        approvals,
    }
}

fn snapshot_live_event(snapshot: RuntimeSnapshot) -> TuiLiveEvent {
    TuiLiveEvent::ReplaceRuntime {
        sessions: snapshot.sessions,
        threads: snapshot.threads,
        items: snapshot.items,
        tasks: snapshot.tasks,
        automations: snapshot.automations,
        usage_summaries: snapshot.usage_summaries,
        approvals: snapshot.approvals,
    }
}

fn start_runtime_live_watcher(
    store: RuntimeStore,
    tx: Sender<TuiLiveEvent>,
    interval: Duration,
) -> RuntimeLiveWatcher {
    let stop = Arc::new(AtomicBool::new(false));
    let worker_stop = stop.clone();
    let mut last_signature = runtime_snapshot(&store)
        .ok()
        .map(|snapshot| runtime_snapshot_signature(&snapshot));
    let handle = thread::spawn(move || {
        while !worker_stop.load(Ordering::Relaxed) {
            thread::sleep(interval);
            if worker_stop.load(Ordering::Relaxed) {
                break;
            }
            match runtime_snapshot(&store) {
                Ok(snapshot) => {
                    let signature = runtime_snapshot_signature(&snapshot);
                    if last_signature.as_ref() == Some(&signature) {
                        continue;
                    }
                    last_signature = Some(signature);
                    if tx.send(snapshot_live_event(snapshot)).is_err() {
                        break;
                    }
                }
                Err(error) => {
                    if tx
                        .send(TuiLiveEvent::Status(format!(
                            "runtime live watcher failed: {error}"
                        )))
                        .is_err()
                    {
                        break;
                    }
                }
            }
        }
    });
    RuntimeLiveWatcher {
        stop,
        handle: Some(handle),
    }
}

fn app_from_store(store: &RuntimeStore) -> AppResult<TuiApp> {
    let snapshot = runtime_snapshot(store)?;
    Ok(TuiApp::with_runtime_usage_tasks_automations_and_approvals(
        snapshot.sessions,
        snapshot.threads,
        snapshot.items,
        snapshot.tasks,
        snapshot.automations,
        snapshot.usage_summaries,
        snapshot.approvals,
    ))
}

fn refresh_app_from_store(store: &RuntimeStore, app: &mut TuiApp) -> AppResult<()> {
    let snapshot = runtime_snapshot(store)?;
    app.replace_runtime_with_usage_tasks_automations_and_approvals(
        snapshot.sessions,
        snapshot.threads,
        snapshot.items,
        snapshot.tasks,
        snapshot.automations,
        snapshot.usage_summaries,
        snapshot.approvals,
    );
    Ok(())
}

fn drain_tui_live_events(rx: &Receiver<TuiLiveEvent>, app: &mut TuiApp) -> AppResult<()> {
    while let Ok(event) = rx.try_recv() {
        app.apply_live_event(event);
    }
    Ok(())
}

#[cfg(test)]
fn handle_tui_action(
    store: &RuntimeStore,
    config: Option<&AppConfig>,
    app: &mut TuiApp,
    action: TuiAction,
) -> AppResult<()> {
    handle_tui_action_with_live(store, config, app, action, None)
}

fn handle_tui_action_with_live(
    store: &RuntimeStore,
    config: Option<&AppConfig>,
    app: &mut TuiApp,
    action: TuiAction,
    live_tx: Option<Sender<TuiLiveEvent>>,
) -> AppResult<()> {
    match action {
        TuiAction::SubmitUserMessage { thread_id, content } => {
            let turn = store.append_turn(&thread_id, "user".to_string(), content.clone())?;
            store.append_item(
                &thread_id,
                Some(&turn.id),
                "message".to_string(),
                Some("user".to_string()),
                content.clone(),
                "completed".to_string(),
            )?;
            if let Some(config) = config {
                start_tui_agent_run(
                    store.clone(),
                    config.clone(),
                    thread_id.clone(),
                    content,
                    live_tx,
                );
                app.set_status(format!("started agent run for {thread_id}"));
            } else {
                app.set_status(format!("submitted user message to {thread_id}"));
            }
        }
        TuiAction::RespondApproval {
            thread_id,
            turn_id,
            request_id,
            decision,
        } => {
            store.append_permission_response(
                &thread_id,
                turn_id.as_deref(),
                request_id.clone(),
                decision.clone(),
            )?;
            app.set_status(format!(
                "recorded approval response: {request_id} {decision}"
            ));
        }
        TuiAction::CancelRun { thread_id, turn_id } => {
            store.append_cancel_request(
                &thread_id,
                turn_id.as_deref(),
                None,
                "user requested cancellation from TUI".to_string(),
            )?;
            app.set_status(format!("cancel event recorded for {thread_id}"));
        }
        TuiAction::CreateTask { thread_id, summary } => {
            let thread = store.load_thread(&thread_id)?;
            let task = store.create_task(
                thread.session_id.as_deref(),
                Some(&thread_id),
                None,
                "agent".to_string(),
                "pending".to_string(),
                summary,
            )?;
            app.set_status(format!("created pending task {}", task.id));
        }
        TuiAction::PauseTask { task_id } => match store.pause_task(&task_id, None) {
            Ok(task) => {
                app.set_status(format!("paused task {}", task.id));
            }
            Err(error) => {
                app.set_status(format!("task pause failed for {task_id}: {error}"));
            }
        },
        TuiAction::ResumeTask { task_id } => match store.resume_task(&task_id, None) {
            Ok(task) => {
                app.set_status(format!("resumed task {}", task.id));
            }
            Err(error) => {
                app.set_status(format!("task resume failed for {task_id}: {error}"));
            }
        },
        TuiAction::RunDiagnostics { changed, paths } => {
            run_tui_diagnostics_from_current_dir(app, changed, paths);
        }
        TuiAction::CreateRollbackSnapshot { label } => {
            let Some(rollback_store) = rollback_store_for_config(config, app) else {
                return Ok(());
            };
            match std::env::current_dir()
                .map_err(|error| app_error(format!("failed to read current directory: {error}")))
                .and_then(|workspace| {
                    rollback_store.create_snapshot(
                        &workspace,
                        label.unwrap_or_else(|| "manual TUI snapshot".to_string()),
                    )
                }) {
                Ok(snapshot) => {
                    app.set_status(format!(
                        "created rollback snapshot {} (patch_bytes={}, untracked_files={})",
                        snapshot.id,
                        snapshot.patch_bytes,
                        snapshot.untracked_files.len()
                    ));
                }
                Err(error) => {
                    app.set_status(format!("rollback snapshot failed: {error}"));
                }
            }
        }
        TuiAction::ListRollbackSnapshots { limit } => {
            let Some(rollback_store) = rollback_store_for_config(config, app) else {
                return Ok(());
            };
            match rollback_store.list_snapshots(limit) {
                Ok(snapshots) if snapshots.is_empty() => {
                    app.set_status("no rollback snapshots".to_string());
                }
                Ok(snapshots) => {
                    let latest = &snapshots[0];
                    app.set_status(format!(
                        "rollback snapshots={} latest={} turn={} {}",
                        snapshots.len(),
                        latest.id,
                        latest.runtime_turn_id.as_deref().unwrap_or("-"),
                        runtime_summary(&latest.label)
                    ));
                }
                Err(error) => {
                    app.set_status(format!("rollback list failed: {error}"));
                }
            }
        }
        TuiAction::ShowRollbackSnapshot { id } => {
            let Some(rollback_store) = rollback_store_for_config(config, app) else {
                return Ok(());
            };
            match rollback_store.load_snapshot_or_turn(&id) {
                Ok(snapshot) => {
                    app.set_status(format!(
                        "rollback snapshot {} patch={} staged={} unstaged={} untracked={}",
                        snapshot.id,
                        snapshot.patch_bytes,
                        snapshot.staged_patch_bytes,
                        snapshot.unstaged_patch_bytes,
                        snapshot.untracked_files.len()
                    ));
                }
                Err(error) => {
                    app.set_status(format!("rollback show failed for {id}: {error}"));
                }
            }
        }
        TuiAction::RevertTurn { id, apply } => {
            let Some(rollback_store) = rollback_store_for_config(config, app) else {
                return Ok(());
            };
            match rollback_store.restore_snapshot(&id, apply) {
                Ok(plan) if plan.applied => {
                    app.set_status(format!(
                        "restored rollback {} changed_files={}",
                        plan.snapshot_id,
                        plan.changed_files.len()
                    ));
                }
                Ok(plan) => {
                    app.set_status(format!(
                        "dry-run rollback {} current_patch={} snapshot_patch={}; add --apply to restore",
                        plan.snapshot_id, plan.current_patch_bytes, plan.patch_bytes
                    ));
                }
                Err(error) => {
                    app.set_status(format!("rollback failed for {id}: {error}"));
                }
            }
        }
        TuiAction::TriggerAutomation {
            automation_id,
            prompt_override,
        } => match store.trigger_automation(&automation_id, prompt_override) {
            Ok((automation, task)) => {
                app.set_status(format!(
                    "triggered automation {} -> task {}",
                    automation.id, task.id
                ));
            }
            Err(error) => {
                app.set_status(format!(
                    "automation trigger failed for {automation_id}: {error}"
                ));
            }
        },
        TuiAction::CompactThread {
            thread_id,
            keep_tail_turns,
        } => match store.compact_thread(&thread_id, keep_tail_turns, None) {
            Ok(compaction) => {
                app.set_status(format!(
                    "compacted {thread_id}: summarized={} kept={}",
                    compaction.summarized_turn_count, compaction.kept_turn_count
                ));
            }
            Err(error) => {
                app.set_status(format!("compaction failed for {thread_id}: {error}"));
            }
        },
    }
    Ok(())
}

fn run_tui_diagnostics_from_current_dir(app: &mut TuiApp, changed: bool, paths: Vec<String>) {
    match std::env::current_dir() {
        Ok(cwd) => run_tui_diagnostics_in(app, &cwd, changed, paths),
        Err(error) => app.set_status(format!("diagnostics failed: {error}")),
    }
}

fn run_tui_diagnostics_in(app: &mut TuiApp, cwd: &Path, changed: bool, paths: Vec<String>) {
    let files = if changed {
        match crate::cli::commands::diagnostics::changed_files(cwd) {
            Ok(files) => files,
            Err(error) => {
                app.set_status(format!("diagnostics changed files failed: {error}"));
                return;
            }
        }
    } else {
        paths
    };
    if changed && files.is_empty() {
        app.set_status("diagnostics: no changed files".to_string());
        return;
    }

    let report = crate::language::diagnostics::run_diagnostics(cwd, &files);
    let engine = if report.engine.is_empty() {
        "none"
    } else {
        report.engine.as_str()
    };
    let target = if report.checked_files.is_empty() {
        if files.is_empty() {
            "workspace".to_string()
        } else {
            format!("{} requested paths", files.len())
        }
    } else {
        format!("{} checked files", report.checked_files.len())
    };
    let mut status = format!(
        "diagnostics {} via {} ({target})",
        report.status.as_str(),
        engine
    );
    if let Some(note) = report.note.as_deref() {
        status.push_str(": ");
        status.push_str(&runtime_summary(note));
    }
    app.set_status(status);
}

fn run_remote_tui_diagnostics(
    client: &RuntimeHttpClient,
    app: &mut TuiApp,
    changed: bool,
    paths: Vec<String>,
) {
    let body = json_object([
        ("changed", JsonValue::Bool(changed)),
        (
            "paths",
            JsonValue::Array(paths.into_iter().map(JsonValue::String).collect()),
        ),
    ]);
    match client.post_json("/v1/diagnostics", body) {
        Ok(value) => app.set_status(remote_diagnostics_status(&value)),
        Err(error) => app.set_status(format!("remote diagnostics failed: {error}")),
    }
}

fn remote_diagnostics_status(value: &JsonValue) -> String {
    let Some(root) = json_as_object(value) else {
        return "remote diagnostics returned malformed response".to_string();
    };
    if matches!(root.get("skipped"), Some(JsonValue::Bool(true))) {
        return root
            .get("message")
            .and_then(json_as_string)
            .map(|message| format!("diagnostics: {message}"))
            .unwrap_or_else(|| "diagnostics skipped".to_string());
    }
    let Some(report) = root.get("report").and_then(json_as_object) else {
        return "remote diagnostics response missing report".to_string();
    };
    let status = report
        .get("status")
        .and_then(json_as_string)
        .unwrap_or("unknown");
    let engine = report
        .get("engine")
        .and_then(json_as_string)
        .filter(|value| !value.is_empty())
        .unwrap_or("none");
    let checked_files = report
        .get("checked_files")
        .and_then(json_as_array)
        .map(|items| items.len())
        .unwrap_or(0);
    let target = if checked_files == 0 {
        "workspace".to_string()
    } else {
        format!("{checked_files} checked files")
    };
    let mut summary = format!("remote diagnostics {status} via {engine} ({target})");
    if let Some(note) = report.get("note").and_then(json_as_string) {
        summary.push_str(": ");
        summary.push_str(&runtime_summary(note));
    }
    summary
}

fn rollback_store_for_config(
    config: Option<&AppConfig>,
    app: &mut TuiApp,
) -> Option<RollbackStore> {
    let Some(config) = config else {
        app.set_status("rollback commands require local TUI config".to_string());
        return None;
    };
    Some(RollbackStore::new(
        PathBuf::from(&config.workspace.config_dir).join("rollback"),
    ))
}

fn start_tui_agent_run(
    store: RuntimeStore,
    config: AppConfig,
    thread_id: String,
    prompt: String,
    live_tx: Option<Sender<TuiLiveEvent>>,
) {
    let _ = thread::spawn(move || {
        if let Err(error) =
            run_tui_agent_turn(store.clone(), config, thread_id.clone(), prompt, live_tx)
        {
            let _ = record_tui_agent_failure(&store, &thread_id, &error.to_string());
        }
    });
}

fn run_tui_agent_turn(
    store: RuntimeStore,
    config: AppConfig,
    thread_id: String,
    prompt: String,
    live_tx: Option<Sender<TuiLiveEvent>>,
) -> AppResult<()> {
    let model = config.model.model.clone();
    let rollback_store =
        RollbackStore::new(PathBuf::from(&config.workspace.config_dir).join("rollback"));
    let rollback_snapshot_id = create_tui_rollback_snapshot(&rollback_store, &prompt);
    let assistant = store.append_turn(
        &thread_id,
        "assistant".to_string(),
        "(assistant response running)".to_string(),
    )?;
    if let Some(snapshot_id) = rollback_snapshot_id.as_deref() {
        let _ = rollback_store.bind_snapshot_runtime(
            snapshot_id,
            Some(&thread_id),
            Some(&assistant.id),
        );
    }
    let assistant_item = store.append_item(
        &thread_id,
        Some(&assistant.id),
        "message".to_string(),
        Some("assistant".to_string()),
        "".to_string(),
        "running".to_string(),
    )?;
    store.update_turn(
        &thread_id,
        &assistant.id,
        "(assistant response running)".to_string(),
        "running".to_string(),
    )?;
    let thread = store.load_thread(&thread_id)?;
    let running_task = store.create_task(
        thread.session_id.as_deref(),
        Some(&thread_id),
        None,
        "agent".to_string(),
        "running".to_string(),
        format!("agent run: {}", runtime_summary(&prompt)),
    )?;
    let cancel_since_seq = store.load_thread(&thread_id)?.event_seq;
    let resolver: SharedAgentApprovalResolver = Rc::new(RefCell::new(RuntimeApprovalResolver {
        store: store.clone(),
        thread_id: thread_id.clone(),
        turn_id: assistant.id.clone(),
        cancel_since_seq,
        poll_interval: Duration::from_millis(250),
    }));
    let cancel_check: SharedAgentCancelCheck = Rc::new(RefCell::new(RuntimeCancelCheck {
        store: store.clone(),
        thread_id: thread_id.clone(),
        turn_id: assistant.id.clone(),
        since_seq: cancel_since_seq,
    }));
    let stream_events = RuntimeItemStream::new(
        store.clone(),
        thread_id.clone(),
        assistant.id.clone(),
        assistant_item.id.clone(),
        live_tx,
    );
    let agent = AgentLoop::new(config);
    let result = match agent.run_with(
        TaskContext::new(prompt, None),
        AgentLoopOptions {
            emit_progress: false,
            persist_session: false,
            stream_events: Some(Box::new(stream_events)),
            approval_resolver: Some(resolver),
            cancel_check: Some(cancel_check),
            ..AgentLoopOptions::default()
        },
    ) {
        Ok(result) => result,
        Err(error) => {
            if is_cancelled_error(&error.to_string()) {
                record_tui_agent_cancelled_into(
                    &store,
                    &thread_id,
                    &assistant.id,
                    &assistant_item.id,
                    Some(&running_task.id),
                )?;
            } else {
                record_tui_agent_failure_into(
                    &store,
                    &thread_id,
                    &assistant.id,
                    &assistant_item.id,
                    Some(&running_task.id),
                    &error.to_string(),
                )?;
            }
            return Ok(());
        }
    };
    record_tui_agent_result_into(
        &store,
        &thread_id,
        &assistant.id,
        &assistant_item.id,
        Some(&running_task.id),
        &model,
        &result,
    )?;
    Ok(())
}

fn create_tui_rollback_snapshot(store: &RollbackStore, prompt: &str) -> Option<String> {
    let workspace = std::env::current_dir().ok()?;
    let label = format!("tui rollback: {}", runtime_summary(prompt));
    store
        .create_snapshot(&workspace, label)
        .ok()
        .map(|snapshot| snapshot.id)
}

struct RuntimeItemStream {
    store: RuntimeStore,
    thread_id: String,
    turn_id: String,
    item_id: String,
    content: String,
    reasoning_item_id: Option<String>,
    reasoning: String,
    live_tx: Option<Sender<TuiLiveEvent>>,
}

impl RuntimeItemStream {
    fn new(
        store: RuntimeStore,
        thread_id: String,
        turn_id: String,
        item_id: String,
        live_tx: Option<Sender<TuiLiveEvent>>,
    ) -> Self {
        Self {
            store,
            thread_id,
            turn_id,
            item_id,
            content: String::new(),
            reasoning_item_id: None,
            reasoning: String::new(),
            live_tx,
        }
    }

    fn flush_running(&self) {
        if let Ok(item) = self.store.update_item(
            &self.thread_id,
            &self.item_id,
            self.content.clone(),
            "running".to_string(),
        ) {
            self.emit_live_item(item);
        }
    }

    fn flush_reasoning(&mut self, status: &str) {
        if self.reasoning.trim().is_empty() {
            return;
        }
        if let Some(item_id) = self.reasoning_item_id.as_deref() {
            if let Ok(item) = self.store.update_item(
                &self.thread_id,
                item_id,
                self.reasoning.clone(),
                status.to_string(),
            ) {
                self.emit_live_item(item);
            }
            return;
        }
        if let Ok(item) = self.store.append_item(
            &self.thread_id,
            Some(&self.turn_id),
            "reasoning".to_string(),
            Some("assistant".to_string()),
            self.reasoning.clone(),
            status.to_string(),
        ) {
            self.reasoning_item_id = Some(item.id.clone());
            self.emit_live_item(item);
        }
    }

    fn emit_live_item(&self, item: crate::core::runtime::ItemRecord) {
        if let Some(tx) = self.live_tx.as_ref() {
            let _ = tx.send(TuiLiveEvent::UpsertItem(TuiItem::from(item)));
        }
    }
}

impl StreamEvents for RuntimeItemStream {
    fn on_reasoning_delta(&mut self, chunk: &str) {
        if chunk.is_empty() {
            return;
        }
        self.reasoning.push_str(chunk);
        self.flush_reasoning("running");
    }

    fn on_text_delta(&mut self, chunk: &str) {
        if chunk.is_empty() {
            return;
        }
        self.content.push_str(chunk);
        self.flush_running();
    }

    fn on_assistant_done(&mut self, full_text: &str) {
        if !full_text.is_empty() {
            self.content = full_text.to_string();
        }
        self.flush_running();
        self.flush_reasoning("completed");
    }

    fn on_tool_call(&mut self, _name: &str, _input: &std::collections::BTreeMap<String, String>) {}
}

struct RuntimeApprovalResolver {
    store: RuntimeStore,
    thread_id: String,
    turn_id: String,
    cancel_since_seq: u64,
    poll_interval: Duration,
}

impl AgentApprovalResolver for RuntimeApprovalResolver {
    fn resolve(&mut self, request: &AgentApprovalRequest) -> AppResult<AgentApprovalDecision> {
        let approval = self.store.append_permission_request(
            &self.thread_id,
            Some(&self.turn_id),
            request.tool_name.clone(),
            request.kind.clone(),
            request.target.clone(),
            request.input.clone(),
        )?;
        loop {
            if runtime_cancel_requested(
                &self.store,
                &self.thread_id,
                &self.turn_id,
                self.cancel_since_seq,
            )? {
                return Err(app_error("agent run cancelled"));
            }
            for event in self.store.read_events(&self.thread_id, approval.seq)? {
                if let Some(decision) = approval_response_decision(&event, &approval.id) {
                    return Ok(decision);
                }
            }
            thread::sleep(self.poll_interval);
        }
    }
}

struct RuntimeCancelCheck {
    store: RuntimeStore,
    thread_id: String,
    turn_id: String,
    since_seq: u64,
}

impl AgentCancelCheck for RuntimeCancelCheck {
    fn is_cancelled(&mut self) -> AppResult<bool> {
        runtime_cancel_requested(&self.store, &self.thread_id, &self.turn_id, self.since_seq)
    }
}

fn runtime_cancel_requested(
    store: &RuntimeStore,
    thread_id: &str,
    turn_id: &str,
    since_seq: u64,
) -> AppResult<bool> {
    Ok(store
        .read_events(thread_id, since_seq)?
        .iter()
        .any(|event| event.kind == "cancel_requested" && event.turn_id.as_deref() == Some(turn_id)))
}

fn approval_response_decision(
    event: &RuntimeEvent,
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

#[cfg(test)]
fn record_tui_agent_result(
    store: &RuntimeStore,
    thread_id: &str,
    model: &str,
    result: &RunResult,
) -> AppResult<()> {
    let message = non_empty_agent_message(&result.final_message);
    let assistant = store.append_turn(thread_id, "assistant".to_string(), message.clone())?;
    let assistant_item = store.append_item(
        thread_id,
        Some(&assistant.id),
        "message".to_string(),
        Some("assistant".to_string()),
        message.clone(),
        "running".to_string(),
    )?;
    record_tui_agent_result_into(
        store,
        thread_id,
        &assistant.id,
        &assistant_item.id,
        None,
        model,
        result,
    )
}

fn record_tui_agent_result_into(
    store: &RuntimeStore,
    thread_id: &str,
    assistant_turn_id: &str,
    assistant_item_id: &str,
    task_id: Option<&str>,
    model: &str,
    result: &RunResult,
) -> AppResult<()> {
    let message = non_empty_agent_message(&result.final_message);
    store.update_turn(
        thread_id,
        assistant_turn_id,
        message.clone(),
        "completed".to_string(),
    )?;
    store.update_item(
        thread_id,
        assistant_item_id,
        message.clone(),
        "completed".to_string(),
    )?;
    for event in &result.tool_events {
        store.append_item(
            thread_id,
            Some(assistant_turn_id),
            "tool_result".to_string(),
            Some("tool".to_string()),
            format_tool_event(event),
            tool_item_status(event),
        )?;
    }
    let usage_model = result.usage.model.as_deref().unwrap_or(model);
    store.append_usage_with_cache(
        thread_id,
        Some(assistant_turn_id),
        usage_model.to_string(),
        "tui".to_string(),
        result.usage.prompt,
        result.usage.completion,
        result.usage.prompt_cache_hit,
        result.usage.prompt_cache_miss,
    )?;
    finish_tui_agent_task(store, thread_id, task_id, "completed", message)?;
    Ok(())
}

fn record_tui_agent_failure(store: &RuntimeStore, thread_id: &str, error: &str) -> AppResult<()> {
    let message = format!("agent run failed: {error}");
    let assistant = store.append_turn(thread_id, "assistant".to_string(), message.clone())?;
    let assistant_item = store.append_item(
        thread_id,
        Some(&assistant.id),
        "message".to_string(),
        Some("assistant".to_string()),
        message.clone(),
        "running".to_string(),
    )?;
    record_tui_agent_failure_into(
        store,
        thread_id,
        &assistant.id,
        &assistant_item.id,
        None,
        error,
    )
}

fn record_tui_agent_failure_into(
    store: &RuntimeStore,
    thread_id: &str,
    assistant_turn_id: &str,
    assistant_item_id: &str,
    task_id: Option<&str>,
    error: &str,
) -> AppResult<()> {
    let message = format!("agent run failed: {error}");
    store.update_turn(
        thread_id,
        assistant_turn_id,
        message.clone(),
        "failed".to_string(),
    )?;
    store.update_item(
        thread_id,
        assistant_item_id,
        message.clone(),
        "failed".to_string(),
    )?;
    finish_tui_agent_task(store, thread_id, task_id, "failed", message)?;
    Ok(())
}

fn record_tui_agent_cancelled_into(
    store: &RuntimeStore,
    thread_id: &str,
    assistant_turn_id: &str,
    assistant_item_id: &str,
    task_id: Option<&str>,
) -> AppResult<()> {
    let running_item = store.load_item(thread_id, assistant_item_id)?;
    let message = if running_item.content.trim().is_empty() {
        "agent run cancelled".to_string()
    } else {
        format!("{}\n\n(agent run cancelled)", running_item.content)
    };
    store.update_turn(
        thread_id,
        assistant_turn_id,
        message.clone(),
        "cancelled".to_string(),
    )?;
    store.update_item(
        thread_id,
        assistant_item_id,
        message.clone(),
        "cancelled".to_string(),
    )?;
    finish_tui_agent_task(store, thread_id, task_id, "cancelled", message)?;
    Ok(())
}

fn finish_tui_agent_task(
    store: &RuntimeStore,
    thread_id: &str,
    task_id: Option<&str>,
    status: &str,
    summary: String,
) -> AppResult<()> {
    if let Some(task_id) = task_id {
        store.update_task(task_id, status.to_string(), summary)?;
        return Ok(());
    }
    let thread = store.load_thread(thread_id)?;
    store.create_task(
        thread.session_id.as_deref(),
        Some(thread_id),
        None,
        "agent".to_string(),
        status.to_string(),
        summary,
    )?;
    Ok(())
}

fn non_empty_agent_message(message: &str) -> String {
    let trimmed = message.trim();
    if trimmed.is_empty() {
        "(assistant finished without a final message)".to_string()
    } else {
        message.to_string()
    }
}

fn runtime_summary(value: &str) -> String {
    let mut summary = value.lines().next().unwrap_or("").trim().to_string();
    if summary.chars().count() > 80 {
        summary = summary.chars().take(80).collect::<String>();
        summary.push_str("...");
    }
    if summary.is_empty() {
        "(empty prompt)".to_string()
    } else {
        summary
    }
}

fn is_cancelled_error(error: &str) -> bool {
    error.contains("agent run cancelled")
}

fn format_tool_event(event: &ToolEvent) -> String {
    let status = match event.status {
        crate::model::protocol::ObservationStatus::Ok => "ok",
        crate::model::protocol::ObservationStatus::Failed => "failed",
    };
    let input = if event.input.is_empty() {
        "{}".to_string()
    } else {
        event
            .input
            .iter()
            .map(|(key, value)| format!("{key}={value}"))
            .collect::<Vec<_>>()
            .join(", ")
    };
    format!(
        "tool: {}\nstatus: {status}\ninput: {input}\n{}",
        event.tool_name, event.output
    )
}

fn tool_item_status(event: &ToolEvent) -> String {
    match event.status {
        crate::model::protocol::ObservationStatus::Ok => "completed".to_string(),
        crate::model::protocol::ObservationStatus::Failed => "failed".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::fs;
    use std::net::TcpListener;
    use std::path::Path;
    use std::process::Command;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_root(label: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "deepseek-tui-runtime-{label}-{}-{nanos}",
            std::process::id()
        ))
    }

    fn temp_store(label: &str) -> RuntimeStore {
        RuntimeStore::new(temp_root(label))
    }

    fn temp_config(root: &Path) -> AppConfig {
        let mut config = AppConfig::default();
        config.workspace.config_dir = root.join(".dscode").display().to_string();
        config
    }

    fn run_git(cwd: &Path, args: &[&str]) {
        let output = Command::new("git")
            .args(args)
            .current_dir(cwd)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn runtime_http_client(
        store: &RuntimeStore,
        request_limit: usize,
    ) -> (RuntimeHttpClient, JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let worker_store = store.clone();
        let handle = thread::spawn(move || {
            crate::cli::commands::serve::serve_http_listener_with_limit(
                listener,
                Some(request_limit),
                &worker_store,
            )
            .unwrap();
        });
        let client = RuntimeHttpClient::from_url(&format!("http://{addr}")).unwrap();
        (client, handle)
    }

    #[test]
    fn app_from_store_loads_session_threads_and_items() {
        let store = temp_store("items");
        let session = store
            .create_session("Daily work".to_string(), ".".to_string())
            .unwrap();
        let thread = store
            .create_thread_for_session(
                &session.id,
                "Runtime timeline".to_string(),
                ".".to_string(),
                "deepseek-coder".to_string(),
                "agent".to_string(),
            )
            .unwrap();
        let turn = store
            .append_turn(&thread.id, "assistant".to_string(), "done".to_string())
            .unwrap();
        store
            .append_item(
                &thread.id,
                Some(&turn.id),
                "message".to_string(),
                Some("assistant".to_string()),
                "done from runtime".to_string(),
                "completed".to_string(),
            )
            .unwrap();

        let app = app_from_store(&store).unwrap();
        let output = render_once(&app, 120, 36).unwrap();

        assert!(output.contains("Daily work"));
        assert!(output.contains("assistant [completed]: done from runtime"));
    }

    #[test]
    fn runtime_live_watcher_emits_snapshot_for_external_runtime_write() {
        let store = temp_store("watcher");
        let session = store
            .create_session("Daily work".to_string(), ".".to_string())
            .unwrap();
        let thread = store
            .create_thread_for_session(
                &session.id,
                "Runtime watcher".to_string(),
                ".".to_string(),
                "deepseek-coder".to_string(),
                "agent".to_string(),
            )
            .unwrap();
        let watched_thread_id = thread.id.clone();
        let (tx, rx) = mpsc::channel();
        let watcher = start_runtime_live_watcher(store.clone(), tx, Duration::from_millis(10));

        let turn = store
            .append_turn(
                &thread.id,
                "user".to_string(),
                "external runtime write".to_string(),
            )
            .unwrap();
        store
            .append_item(
                &thread.id,
                Some(&turn.id),
                "message".to_string(),
                Some("user".to_string()),
                "external runtime write".to_string(),
                "completed".to_string(),
            )
            .unwrap();

        let mut matched = false;
        for _ in 0..10 {
            let event = rx.recv_timeout(Duration::from_millis(100)).unwrap();
            if let TuiLiveEvent::ReplaceRuntime { items, threads, .. } = event {
                matched = threads
                    .iter()
                    .any(|thread| thread.id == watched_thread_id && thread.event_seq >= 3)
                    && items
                        .iter()
                        .any(|item| item.content == "external runtime write");
                if matched {
                    break;
                }
            }
        }
        drop(watcher);
        assert!(matched);
    }

    #[test]
    fn runtime_http_snapshot_loads_remote_runtime() {
        let store = temp_store("http-snapshot");
        let session = store
            .create_session("Remote work".to_string(), ".".to_string())
            .unwrap();
        let thread = store
            .create_thread_for_session(
                &session.id,
                "Remote timeline".to_string(),
                ".".to_string(),
                "deepseek-coder".to_string(),
                "agent".to_string(),
            )
            .unwrap();
        let turn = store
            .append_turn(
                &thread.id,
                "assistant".to_string(),
                "remote done".to_string(),
            )
            .unwrap();
        store
            .append_item(
                &thread.id,
                Some(&turn.id),
                "message".to_string(),
                Some("assistant".to_string()),
                "remote done".to_string(),
                "completed".to_string(),
            )
            .unwrap();
        store
            .create_task(
                Some(&session.id),
                Some(&thread.id),
                None,
                "agent".to_string(),
                "running".to_string(),
                "agent run: remote snapshot".to_string(),
            )
            .unwrap();
        store
            .create_automation(
                Some(&session.id),
                Some(&thread.id),
                "Remote automation".to_string(),
                "active".to_string(),
                "manual".to_string(),
                "run remote task".to_string(),
                None,
                None,
            )
            .unwrap();
        store
            .append_usage_with_cache(
                &thread.id,
                Some(&turn.id),
                "deepseek-v4-flash".to_string(),
                "remote".to_string(),
                12,
                3,
                7,
                5,
            )
            .unwrap();
        store
            .append_permission_request(
                &thread.id,
                Some(&turn.id),
                "run_shell".to_string(),
                "shell".to_string(),
                "cargo test".to_string(),
                BTreeMap::new(),
            )
            .unwrap();

        let (client, handle) = runtime_http_client(&store, 7);
        let snapshot = runtime_http_snapshot(&client).unwrap();
        handle.join().unwrap();

        assert_eq!(snapshot.sessions[0].title, "Remote work");
        assert_eq!(snapshot.threads[0].title, "Remote timeline");
        assert!(snapshot
            .items
            .iter()
            .any(|item| item.content == "remote done"));
        assert_eq!(snapshot.tasks.len(), 1);
        assert_eq!(snapshot.automations.len(), 1);
        assert_eq!(snapshot.usage_summaries[0].total_tokens, 15);
        assert_eq!(snapshot.approvals.len(), 1);
    }

    #[test]
    fn handle_tui_http_action_submits_remote_user_turn() {
        let store = temp_store("http-action");
        let session = store
            .create_session("Remote action".to_string(), ".".to_string())
            .unwrap();
        let thread = store
            .create_thread_for_session(
                &session.id,
                "Remote action thread".to_string(),
                ".".to_string(),
                "deepseek-coder".to_string(),
                "agent".to_string(),
            )
            .unwrap();
        let (client, handle) = runtime_http_client(&store, 1);
        let mut app = TuiApp::with_runtime(
            vec![TuiSession::from(session)],
            vec![TuiThread::from(thread.clone())],
            Vec::new(),
        );

        handle_tui_http_action(
            &client,
            &mut app,
            TuiAction::SubmitUserMessage {
                thread_id: thread.id.clone(),
                content: "hello remote runtime".to_string(),
            },
        )
        .unwrap();
        handle.join().unwrap();

        let turns = store.list_turns(&thread.id).unwrap();
        assert!(turns
            .iter()
            .any(|turn| turn.role == "user" && turn.content == "hello remote runtime"));
    }

    #[test]
    fn app_from_store_loads_usage_summary_into_task_panel() {
        let store = temp_store("usage");
        let session = store
            .create_session("Cost watch".to_string(), ".".to_string())
            .unwrap();
        let thread = store
            .create_thread_for_session(
                &session.id,
                "Runtime usage".to_string(),
                ".".to_string(),
                "deepseek-v4-flash".to_string(),
                "agent".to_string(),
            )
            .unwrap();
        let turn = store
            .append_turn(&thread.id, "assistant".to_string(), "done".to_string())
            .unwrap();
        store
            .append_usage_with_cache(
                &thread.id,
                Some(&turn.id),
                "deepseek-v4-flash".to_string(),
                "tui".to_string(),
                12,
                3,
                7,
                5,
            )
            .unwrap();

        let app = app_from_store(&store).unwrap();
        let output = render_once(&app, 160, 48).unwrap();

        assert!(output.contains("Usage total: 15 tokens"));
        assert!(output.contains("Cache hit: 7 / 12"));
        assert!(output.contains("Cache chart: ["));
        assert!(output.contains("Est. cost: $0.000002"));
        assert!(output.contains("Cost split: in"));
        assert!(output.contains("Cost chart: ["));
    }

    #[test]
    fn app_from_store_loads_runtime_tasks_into_task_panel() {
        let store = temp_store("tasks");
        let session = store
            .create_session("Progress watch".to_string(), ".".to_string())
            .unwrap();
        let thread = store
            .create_thread_for_session(
                &session.id,
                "Runtime tasks".to_string(),
                ".".to_string(),
                "deepseek-coder".to_string(),
                "agent".to_string(),
            )
            .unwrap();
        store
            .create_task(
                Some(&session.id),
                Some(&thread.id),
                None,
                "agent".to_string(),
                "running".to_string(),
                "agent run: stream progress".to_string(),
            )
            .unwrap();

        let app = app_from_store(&store).unwrap();
        let output = render_once(&app, 160, 48).unwrap();

        assert!(output.contains("Runtime tasks: 1"));
        assert!(output.contains("Task agent [running]"));
    }

    #[test]
    fn app_from_store_loads_automations_into_task_panel() {
        let store = temp_store("automations");
        let session = store
            .create_session("Automation watch".to_string(), ".".to_string())
            .unwrap();
        let thread = store
            .create_thread_for_session(
                &session.id,
                "Runtime automations".to_string(),
                ".".to_string(),
                "deepseek-coder".to_string(),
                "agent".to_string(),
            )
            .unwrap();
        store
            .create_automation(
                Some(&session.id),
                Some(&thread.id),
                "Nightly diagnostics".to_string(),
                "active".to_string(),
                "daily".to_string(),
                "run diagnostics".to_string(),
                None,
                Some("epoch+100".to_string()),
            )
            .unwrap();

        let app = app_from_store(&store).unwrap();
        let output = render_once(&app, 160, 48).unwrap();

        assert!(output.contains("Automations: 1"));
        assert!(output.contains("Automation Nightly"));
    }

    #[test]
    fn refresh_app_from_store_updates_existing_app() {
        let store = temp_store("refresh");
        let session = store
            .create_session("Daily work".to_string(), ".".to_string())
            .unwrap();
        let thread = store
            .create_thread_for_session(
                &session.id,
                "Runtime timeline".to_string(),
                ".".to_string(),
                "deepseek-coder".to_string(),
                "agent".to_string(),
            )
            .unwrap();
        let mut app = app_from_store(&store).unwrap();
        assert!(render_once(&app, 120, 36)
            .unwrap()
            .contains("No durable items recorded"));

        let turn = store
            .append_turn(&thread.id, "assistant".to_string(), "done".to_string())
            .unwrap();
        store
            .append_item(
                &thread.id,
                Some(&turn.id),
                "message".to_string(),
                Some("assistant".to_string()),
                "fresh runtime item".to_string(),
                "completed".to_string(),
            )
            .unwrap();

        refresh_app_from_store(&store, &mut app).unwrap();
        let output = render_once(&app, 120, 36).unwrap();

        assert!(output.contains("assistant [completed]: fresh runtime item"));
        assert!(output.contains("runtime refreshed: sessions=1 threads=1 items=1"));
    }

    #[test]
    fn app_from_store_loads_permission_request_events() {
        let store = temp_store("approval");
        let session = store
            .create_session("Daily work".to_string(), ".".to_string())
            .unwrap();
        let thread = store
            .create_thread_for_session(
                &session.id,
                "Runtime permissions".to_string(),
                ".".to_string(),
                "deepseek-coder".to_string(),
                "agent".to_string(),
            )
            .unwrap();
        let mut input = BTreeMap::new();
        input.insert("command".to_string(), "cargo test".to_string());
        store
            .append_permission_request(
                &thread.id,
                None,
                "run_shell".to_string(),
                "shell".to_string(),
                "cargo test".to_string(),
                input,
            )
            .unwrap();

        let app = app_from_store(&store).unwrap();
        let output = render_once(&app, 120, 36).unwrap();

        assert!(output.contains("Approval Modal"));
        assert!(output.contains("run_shell"));
        assert!(output.contains("cargo test"));
    }

    #[test]
    fn app_from_store_hides_answered_permission_request_events() {
        let store = temp_store("approval-answered");
        let session = store
            .create_session("Daily work".to_string(), ".".to_string())
            .unwrap();
        let thread = store
            .create_thread_for_session(
                &session.id,
                "Runtime permissions".to_string(),
                ".".to_string(),
                "deepseek-coder".to_string(),
                "agent".to_string(),
            )
            .unwrap();
        let request = store
            .append_permission_request(
                &thread.id,
                None,
                "run_shell".to_string(),
                "shell".to_string(),
                "cargo test".to_string(),
                BTreeMap::new(),
            )
            .unwrap();
        store
            .append_permission_response(&thread.id, None, request.id, "denied".to_string())
            .unwrap();

        let app = app_from_store(&store).unwrap();
        let output = render_once(&app, 120, 36).unwrap();

        assert!(!output.contains("Approval Modal"));
        assert!(!output.contains("cargo test"));
    }

    #[test]
    fn handle_tui_action_records_composer_message() {
        let store = temp_store("composer");
        let session = store
            .create_session("Daily work".to_string(), ".".to_string())
            .unwrap();
        let thread = store
            .create_thread_for_session(
                &session.id,
                "Runtime composer".to_string(),
                ".".to_string(),
                "deepseek-coder".to_string(),
                "agent".to_string(),
            )
            .unwrap();
        let mut app = app_from_store(&store).unwrap();

        handle_tui_action(
            &store,
            None,
            &mut app,
            TuiAction::SubmitUserMessage {
                thread_id: thread.id.clone(),
                content: "hello from composer".to_string(),
            },
        )
        .unwrap();
        refresh_app_from_store(&store, &mut app).unwrap();
        let output = render_once(&app, 120, 36).unwrap();

        assert!(output.contains("user [completed]: hello from composer"));
        let turns = store.list_turns(&thread.id).unwrap();
        assert_eq!(turns.len(), 1);
        assert_eq!(turns[0].role, "user");
        assert_eq!(store.list_items(&thread.id, None).unwrap().len(), 1);
    }

    #[test]
    fn handle_tui_action_records_approval_response() {
        let store = temp_store("approval-response");
        let session = store
            .create_session("Daily work".to_string(), ".".to_string())
            .unwrap();
        let thread = store
            .create_thread_for_session(
                &session.id,
                "Runtime permissions".to_string(),
                ".".to_string(),
                "deepseek-coder".to_string(),
                "agent".to_string(),
            )
            .unwrap();
        let request = store
            .append_permission_request(
                &thread.id,
                None,
                "run_shell".to_string(),
                "shell".to_string(),
                "cargo test".to_string(),
                BTreeMap::new(),
            )
            .unwrap();
        let mut app = app_from_store(&store).unwrap();

        handle_tui_action(
            &store,
            None,
            &mut app,
            TuiAction::RespondApproval {
                thread_id: thread.id.clone(),
                turn_id: None,
                request_id: request.id.clone(),
                decision: "approved".to_string(),
            },
        )
        .unwrap();
        refresh_app_from_store(&store, &mut app).unwrap();

        let events = store.read_events(&thread.id, 0).unwrap();
        assert_eq!(events.last().unwrap().kind, "permission_response");
        let output = render_once(&app, 120, 36).unwrap();
        assert!(!output.contains("Approval Modal"));
        assert!(!output.contains("cargo test"));
    }

    #[test]
    fn handle_tui_action_records_cancel_request() {
        let store = temp_store("cancel-action");
        let session = store
            .create_session("Daily work".to_string(), ".".to_string())
            .unwrap();
        let thread = store
            .create_thread_for_session(
                &session.id,
                "Runtime cancel".to_string(),
                ".".to_string(),
                "deepseek-coder".to_string(),
                "agent".to_string(),
            )
            .unwrap();
        let turn = store
            .append_turn(&thread.id, "assistant".to_string(), "running".to_string())
            .unwrap();
        let mut app = app_from_store(&store).unwrap();

        handle_tui_action(
            &store,
            None,
            &mut app,
            TuiAction::CancelRun {
                thread_id: thread.id.clone(),
                turn_id: Some(turn.id.clone()),
            },
        )
        .unwrap();

        let events = store.read_events(&thread.id, 0).unwrap();
        assert_eq!(events.last().unwrap().kind, "cancel_requested");
        assert_eq!(
            events.last().unwrap().turn_id.as_deref(),
            Some(turn.id.as_str())
        );
        assert!(runtime_cancel_requested(&store, &thread.id, &turn.id, 0).unwrap());
    }

    #[test]
    fn handle_tui_action_creates_pending_runtime_task() {
        let store = temp_store("create-task-action");
        let session = store
            .create_session("Daily work".to_string(), ".".to_string())
            .unwrap();
        let thread = store
            .create_thread_for_session(
                &session.id,
                "Runtime tasks".to_string(),
                ".".to_string(),
                "deepseek-coder".to_string(),
                "agent".to_string(),
            )
            .unwrap();
        let mut app = app_from_store(&store).unwrap();

        handle_tui_action(
            &store,
            None,
            &mut app,
            TuiAction::CreateTask {
                thread_id: thread.id.clone(),
                summary: "inspect flaky test".to_string(),
            },
        )
        .unwrap();

        let tasks = store
            .list_tasks(Some(&session.id), Some(&thread.id), 10)
            .unwrap();
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].kind, "agent");
        assert_eq!(tasks[0].status, "pending");
        assert_eq!(tasks[0].summary, "inspect flaky test");
        assert!(store
            .read_events(&thread.id, 0)
            .unwrap()
            .iter()
            .any(|event| event.kind == "task_recorded"));
        let output = render_once(&app, 160, 48).unwrap();
        assert!(output.contains("created pending task"));
    }

    #[test]
    fn handle_tui_action_pauses_and_resumes_runtime_task() {
        let store = temp_store("pause-resume-task-action");
        let session = store
            .create_session("Daily work".to_string(), ".".to_string())
            .unwrap();
        let thread = store
            .create_thread_for_session(
                &session.id,
                "Runtime tasks".to_string(),
                ".".to_string(),
                "deepseek-coder".to_string(),
                "agent".to_string(),
            )
            .unwrap();
        let task = store
            .create_task(
                Some(&session.id),
                Some(&thread.id),
                None,
                "agent".to_string(),
                "pending".to_string(),
                "inspect flaky test".to_string(),
            )
            .unwrap();
        let mut app = app_from_store(&store).unwrap();

        handle_tui_action(
            &store,
            None,
            &mut app,
            TuiAction::PauseTask {
                task_id: task.id.clone(),
            },
        )
        .unwrap();
        assert_eq!(store.load_task(&task.id).unwrap().status, "paused");
        assert!(render_once(&app, 160, 48).unwrap().contains("paused task"));

        handle_tui_action(
            &store,
            None,
            &mut app,
            TuiAction::ResumeTask {
                task_id: task.id.clone(),
            },
        )
        .unwrap();
        assert_eq!(store.load_task(&task.id).unwrap().status, "pending");
        assert!(render_once(&app, 160, 48).unwrap().contains("resumed task"));
    }

    #[test]
    fn handle_tui_action_lists_shows_and_restores_rollback_snapshot() {
        let repo = temp_root("rollback-action");
        fs::create_dir_all(&repo).unwrap();
        run_git(&repo, &["init"]);
        run_git(&repo, &["config", "user.email", "test@example.com"]);
        run_git(&repo, &["config", "user.name", "Deepseek Test"]);
        fs::write(repo.join("src.txt"), "base\n").unwrap();
        run_git(&repo, &["add", "src.txt"]);
        run_git(&repo, &["commit", "-m", "initial"]);

        fs::write(repo.join("src.txt"), "snapshot\n").unwrap();
        let config = temp_config(&repo);
        let rollback_store =
            RollbackStore::new(PathBuf::from(&config.workspace.config_dir).join("rollback"));
        let snapshot = rollback_store
            .create_snapshot(&repo, "before TUI turn".to_string())
            .unwrap();
        rollback_store
            .bind_snapshot_runtime(&snapshot.id, Some("thread-one"), Some("turn-one"))
            .unwrap();
        fs::write(repo.join("src.txt"), "after\n").unwrap();

        let store = RuntimeStore::new(repo.join(".dscode/runtime"));
        let mut app = TuiApp::new(Vec::new());

        handle_tui_action(
            &store,
            Some(&config),
            &mut app,
            TuiAction::ListRollbackSnapshots { limit: 20 },
        )
        .unwrap();
        assert!(render_once(&app, 160, 48)
            .unwrap()
            .contains("rollback snapshots=1"));

        handle_tui_action(
            &store,
            Some(&config),
            &mut app,
            TuiAction::ShowRollbackSnapshot {
                id: "turn-one".to_string(),
            },
        )
        .unwrap();
        assert!(render_once(&app, 160, 48)
            .unwrap()
            .contains(snapshot.id.as_str()));

        handle_tui_action(
            &store,
            Some(&config),
            &mut app,
            TuiAction::RevertTurn {
                id: "turn-one".to_string(),
                apply: false,
            },
        )
        .unwrap();
        assert_eq!(fs::read_to_string(repo.join("src.txt")).unwrap(), "after\n");
        assert!(render_once(&app, 160, 48)
            .unwrap()
            .contains("dry-run rollback"));

        handle_tui_action(
            &store,
            Some(&config),
            &mut app,
            TuiAction::RevertTurn {
                id: "turn-one".to_string(),
                apply: true,
            },
        )
        .unwrap();
        assert_eq!(
            fs::read_to_string(repo.join("src.txt")).unwrap(),
            "snapshot\n"
        );
        assert!(render_once(&app, 160, 48)
            .unwrap()
            .contains("restored rollback"));
    }

    #[test]
    fn run_tui_diagnostics_reports_status() {
        let root = temp_root("diagnostics-action");
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("README.md"), "# docs\n").unwrap();
        let mut app = TuiApp::new(Vec::new());

        run_tui_diagnostics_in(&mut app, &root, false, vec!["README.md".to_string()]);

        assert!(render_once(&app, 160, 48).unwrap().contains("diagnostics"));
    }

    #[test]
    fn remote_diagnostics_status_summarizes_runtime_report() {
        let value = json_object([
            (
                "schema",
                JsonValue::String("deepseek.runtime.diagnostics.v1".to_string()),
            ),
            ("skipped", JsonValue::Bool(false)),
            (
                "report",
                json_object([
                    ("status", JsonValue::String("passed".to_string())),
                    (
                        "engine",
                        JsonValue::String("lsp publishDiagnostics".to_string()),
                    ),
                    (
                        "checked_files",
                        JsonValue::Array(vec![JsonValue::String("src/lib.rs".to_string())]),
                    ),
                    ("note", JsonValue::Null),
                ]),
            ),
        ]);

        assert_eq!(
            remote_diagnostics_status(&value),
            "remote diagnostics passed via lsp publishDiagnostics (1 checked files)"
        );
    }

    #[test]
    fn handle_tui_action_triggers_automation_into_pending_task() {
        let store = temp_store("automation-trigger-action");
        let session = store
            .create_session("Daily work".to_string(), ".".to_string())
            .unwrap();
        let thread = store
            .create_thread_for_session(
                &session.id,
                "Runtime automation".to_string(),
                ".".to_string(),
                "deepseek-coder".to_string(),
                "agent".to_string(),
            )
            .unwrap();
        let automation = store
            .create_automation(
                Some(&session.id),
                Some(&thread.id),
                "Nightly diagnostics".to_string(),
                "active".to_string(),
                "daily".to_string(),
                "run diagnostics".to_string(),
                None,
                Some("epoch+100".to_string()),
            )
            .unwrap();
        let mut app = app_from_store(&store).unwrap();

        handle_tui_action(
            &store,
            None,
            &mut app,
            TuiAction::TriggerAutomation {
                automation_id: automation.id.clone(),
                prompt_override: Some("manual run now".to_string()),
            },
        )
        .unwrap();

        let tasks = store
            .list_tasks(Some(&session.id), Some(&thread.id), 10)
            .unwrap();
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].kind, "automation");
        assert_eq!(tasks[0].status, "pending");
        assert_eq!(tasks[0].summary, "manual run now");
        let events = store.read_events(&thread.id, 0).unwrap();
        assert!(events
            .iter()
            .any(|event| event.kind == "automation_triggered"));
        let output = render_once(&app, 160, 48).unwrap();
        assert!(output.contains("triggered automation"));
    }

    #[test]
    fn handle_tui_action_compacts_active_thread() {
        let store = temp_store("compact-action");
        let session = store
            .create_session("Daily work".to_string(), ".".to_string())
            .unwrap();
        let thread = store
            .create_thread_for_session(
                &session.id,
                "Runtime compaction".to_string(),
                ".".to_string(),
                "deepseek-coder".to_string(),
                "agent".to_string(),
            )
            .unwrap();
        for content in ["first", "second", "third"] {
            let turn = store
                .append_turn(&thread.id, "user".to_string(), content.to_string())
                .unwrap();
            store
                .append_item(
                    &thread.id,
                    Some(&turn.id),
                    "message".to_string(),
                    Some("user".to_string()),
                    content.to_string(),
                    "completed".to_string(),
                )
                .unwrap();
        }
        let mut app = app_from_store(&store).unwrap();

        handle_tui_action(
            &store,
            None,
            &mut app,
            TuiAction::CompactThread {
                thread_id: thread.id.clone(),
                keep_tail_turns: 1,
            },
        )
        .unwrap();

        let refreshed = app_from_store(&store).unwrap();
        let output = render_once(&refreshed, 160, 48).unwrap();
        assert!(output.contains("Compacted runtime thread summary"));
        let events = store.read_events(&thread.id, 0).unwrap();
        assert_eq!(events.last().unwrap().kind, "thread_compacted");
        let action_output = render_once(&app, 160, 48).unwrap();
        assert!(action_output.contains("compacted"));
        assert!(action_output.contains("summarized=2"));
    }

    #[test]
    fn record_tui_agent_result_appends_assistant_tools_usage_and_task() {
        let store = temp_store("agent-result");
        let session = store
            .create_session("Daily work".to_string(), ".".to_string())
            .unwrap();
        let thread = store
            .create_thread_for_session(
                &session.id,
                "Runtime agent".to_string(),
                ".".to_string(),
                "deepseek-coder".to_string(),
                "agent".to_string(),
            )
            .unwrap();
        let mut usage = crate::model::protocol::TokenUsage::new(3, 4);
        usage.model = Some("deepseek-v4-flash".to_string());
        let result = RunResult {
            final_message: "done from agent".to_string(),
            tool_events: vec![ToolEvent {
                tool_name: "run_shell".to_string(),
                input: BTreeMap::from([("command".to_string(), "pwd".to_string())]),
                output: "exit_code: 0".to_string(),
                status: crate::model::protocol::ObservationStatus::Ok,
            }],
            usage,
        };

        record_tui_agent_result(&store, &thread.id, "deepseek-coder", &result).unwrap();

        let items = store.list_items(&thread.id, None).unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].role.as_deref(), Some("assistant"));
        assert_eq!(items[0].content, "done from agent");
        assert_eq!(items[1].item_type, "tool_result");
        assert!(items[1].content.contains("tool: run_shell"));
        assert!(items[1].content.contains("command=pwd"));
        let usage = store.list_usage(Some(&thread.id), 10).unwrap();
        assert_eq!(usage.len(), 1);
        assert_eq!(usage[0].source, "tui");
        assert_eq!(usage[0].model, "deepseek-v4-flash");
        assert_eq!(usage[0].total_tokens, 7);
        let tasks = store.list_tasks(None, Some(&thread.id), 10).unwrap();
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].status, "completed");
    }

    #[test]
    fn record_tui_agent_cancelled_surfaces_cancelled_item_and_task() {
        let store = temp_store("agent-cancelled");
        let session = store
            .create_session("Daily work".to_string(), ".".to_string())
            .unwrap();
        let thread = store
            .create_thread_for_session(
                &session.id,
                "Runtime agent".to_string(),
                ".".to_string(),
                "deepseek-coder".to_string(),
                "agent".to_string(),
            )
            .unwrap();
        let turn = store
            .append_turn(&thread.id, "assistant".to_string(), "running".to_string())
            .unwrap();
        let item = store
            .append_item(
                &thread.id,
                Some(&turn.id),
                "message".to_string(),
                Some("assistant".to_string()),
                "partial".to_string(),
                "running".to_string(),
            )
            .unwrap();
        let task = store
            .create_task(
                Some(&session.id),
                Some(&thread.id),
                None,
                "agent".to_string(),
                "running".to_string(),
                "agent run: test".to_string(),
            )
            .unwrap();

        record_tui_agent_cancelled_into(&store, &thread.id, &turn.id, &item.id, Some(&task.id))
            .unwrap();

        let item = store.load_item(&thread.id, &item.id).unwrap();
        assert_eq!(item.status, "cancelled");
        assert!(item.content.contains("partial"));
        assert!(item.content.contains("agent run cancelled"));
        assert_eq!(store.load_task(&task.id).unwrap().status, "cancelled");
    }

    #[test]
    fn runtime_item_stream_updates_running_assistant_item() {
        let store = temp_store("stream");
        let session = store
            .create_session("Daily work".to_string(), ".".to_string())
            .unwrap();
        let thread = store
            .create_thread_for_session(
                &session.id,
                "Runtime stream".to_string(),
                ".".to_string(),
                "deepseek-coder".to_string(),
                "agent".to_string(),
            )
            .unwrap();
        let turn = store
            .append_turn(
                &thread.id,
                "assistant".to_string(),
                "(assistant response running)".to_string(),
            )
            .unwrap();
        let item = store
            .append_item(
                &thread.id,
                Some(&turn.id),
                "message".to_string(),
                Some("assistant".to_string()),
                "".to_string(),
                "running".to_string(),
            )
            .unwrap();
        let mut stream = RuntimeItemStream::new(
            store.clone(),
            thread.id.clone(),
            turn.id.clone(),
            item.id.clone(),
            None,
        );

        stream.on_reasoning_delta("thinking");
        stream.on_text_delta("hello");
        stream.on_text_delta(" world");
        stream.on_assistant_done("hello world");

        let running = store.load_item(&thread.id, &item.id).unwrap();
        assert_eq!(running.content, "hello world");
        assert_eq!(running.status, "running");
        let items = store.list_items(&thread.id, Some(&turn.id)).unwrap();
        let reasoning = items
            .iter()
            .find(|item| item.item_type == "reasoning")
            .expect("reasoning item");
        assert_eq!(reasoning.content, "thinking");
        assert_eq!(reasoning.status, "completed");
        assert!(store
            .read_events(&thread.id, 0)
            .unwrap()
            .iter()
            .any(|event| event.kind == "item_updated"));
    }

    #[test]
    fn runtime_item_stream_emits_live_item_updates() {
        let store = temp_store("stream-live");
        let session = store
            .create_session("Daily work".to_string(), ".".to_string())
            .unwrap();
        let thread = store
            .create_thread_for_session(
                &session.id,
                "Runtime stream".to_string(),
                ".".to_string(),
                "deepseek-coder".to_string(),
                "agent".to_string(),
            )
            .unwrap();
        let turn = store
            .append_turn(
                &thread.id,
                "assistant".to_string(),
                "(assistant response running)".to_string(),
            )
            .unwrap();
        let item = store
            .append_item(
                &thread.id,
                Some(&turn.id),
                "message".to_string(),
                Some("assistant".to_string()),
                "".to_string(),
                "running".to_string(),
            )
            .unwrap();
        let (tx, rx) = mpsc::channel();
        let mut stream = RuntimeItemStream::new(
            store.clone(),
            thread.id.clone(),
            turn.id.clone(),
            item.id.clone(),
            Some(tx),
        );

        stream.on_text_delta("hello");
        stream.on_text_delta(" live");
        stream.on_reasoning_delta("thinking");

        let events = rx.try_iter().collect::<Vec<_>>();
        assert!(events.iter().any(|event| matches!(
            event,
            TuiLiveEvent::UpsertItem(item)
                if item.item_type == "message" && item.content == "hello live"
        )));
        assert!(events.iter().any(|event| matches!(
            event,
            TuiLiveEvent::UpsertItem(item)
                if item.item_type == "reasoning" && item.content == "thinking"
        )));
    }

    #[test]
    fn record_tui_agent_failure_surfaces_failed_assistant_item_and_task() {
        let store = temp_store("agent-failure");
        let session = store
            .create_session("Daily work".to_string(), ".".to_string())
            .unwrap();
        let thread = store
            .create_thread_for_session(
                &session.id,
                "Runtime agent".to_string(),
                ".".to_string(),
                "deepseek-coder".to_string(),
                "agent".to_string(),
            )
            .unwrap();

        record_tui_agent_failure(&store, &thread.id, "missing api key").unwrap();

        let items = store.list_items(&thread.id, None).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].status, "failed");
        assert!(items[0]
            .content
            .contains("agent run failed: missing api key"));
        let tasks = store.list_tasks(None, Some(&thread.id), 10).unwrap();
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].status, "failed");
    }

    #[test]
    fn approval_response_decision_matches_request_id() {
        let store = temp_store("approval-decision");
        let session = store
            .create_session("Daily work".to_string(), ".".to_string())
            .unwrap();
        let thread = store
            .create_thread_for_session(
                &session.id,
                "Runtime permissions".to_string(),
                ".".to_string(),
                "deepseek-coder".to_string(),
                "agent".to_string(),
            )
            .unwrap();
        let request = store
            .append_permission_request(
                &thread.id,
                None,
                "run_shell".to_string(),
                "shell".to_string(),
                "pwd".to_string(),
                BTreeMap::new(),
            )
            .unwrap();
        store
            .append_permission_response(
                &thread.id,
                None,
                request.id.clone(),
                "approved".to_string(),
            )
            .unwrap();

        let events = store.read_events(&thread.id, request.seq).unwrap();
        assert_eq!(
            events
                .iter()
                .find_map(|event| approval_response_decision(event, &request.id)),
            Some(AgentApprovalDecision::Approved)
        );
        assert_eq!(
            events
                .iter()
                .find_map(|event| approval_response_decision(event, "event-other")),
            None
        );
    }
}
