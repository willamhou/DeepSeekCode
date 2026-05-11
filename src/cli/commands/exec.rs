use std::cell::RefCell;
use std::collections::BTreeMap;
use std::fs;
use std::io::{self, Read};
use std::path::PathBuf;
use std::rc::Rc;

use crate::cli::app::{ExecAction, ExecArgs, ExecResumeArgs};
use crate::config::load::load_or_default;
use crate::config::types::AppConfig;
use crate::core::context::TaskContext;
use crate::core::loop_runtime::{
    AgentLoop, AgentLoopOptions, AgentRunEvents, RunResult, SharedAgentRunEvents, ToolEvent,
};
use crate::core::rollback::RollbackStore;
use crate::core::runtime::RuntimeStore;
use crate::core::session::{SessionSnapshot, SessionStore};
use crate::error::{app_error, AppResult};
use crate::model::protocol::{ImageInput, ObservationStatus};
use crate::ui::stream::StreamEvents;
use crate::util::json::{json_value_to_string, JsonValue};

pub fn run(action: ExecAction) -> AppResult<()> {
    let config = load_or_default()?;
    match action {
        ExecAction::Run(args) => run_exec(config, args),
        ExecAction::Resume(args) => run_exec_resume(config, args),
    }
}

fn run_exec(config: AppConfig, args: ExecArgs) -> AppResult<()> {
    let task = resolve_prompt_argument(&args.task)?;
    let image_inputs = load_image_inputs(&args.images)?;
    let task = task_with_image_references(task, &image_inputs);
    run_task(
        config,
        task,
        args.skill,
        args.budget,
        args.json,
        image_inputs,
        None,
    )
}

fn run_exec_resume(config: AppConfig, args: ExecResumeArgs) -> AppResult<()> {
    let store = SessionStore::new(config.workspace.session_dir());
    let snapshot = store.load_latest(args.session.as_deref())?;
    let followup = args
        .task
        .as_deref()
        .map(resolve_prompt_argument)
        .transpose()?;
    let task = build_resume_task(&snapshot, followup.as_deref());
    let image_inputs = load_image_inputs(&args.images)?;
    let task = task_with_image_references(task, &image_inputs);
    run_task(
        config,
        task,
        args.skill,
        args.budget,
        args.json,
        image_inputs,
        Some(snapshot.id),
    )
}

fn task_with_image_references(task: String, images: &[ImageInput]) -> String {
    if images.is_empty() {
        return task;
    }

    format!(
        "{task}\n\nAttached image files:\n{}",
        images
            .iter()
            .map(|image| format!("- {} ({})", image.path, image.media_type))
            .collect::<Vec<_>>()
            .join("\n")
    )
}

fn load_image_inputs(images: &[String]) -> AppResult<Vec<ImageInput>> {
    let mut loaded = Vec::with_capacity(images.len());
    for image in images {
        let path = std::path::Path::new(image);
        if !path.is_file() {
            return Err(app_error(format!("image input not found: {image}")));
        }
        let media_type = image_media_type(path)?;
        let bytes = fs::read(path)?;
        loaded.push(ImageInput {
            path: image.to_string(),
            media_type: media_type.to_string(),
            data_base64: encode_base64(&bytes),
        });
    }
    Ok(loaded)
}

fn image_media_type(path: &std::path::Path) -> AppResult<&'static str> {
    let ext = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match ext.as_str() {
        "jpg" | "jpeg" => Ok("image/jpeg"),
        "png" => Ok("image/png"),
        "gif" => Ok("image/gif"),
        "webp" => Ok("image/webp"),
        _ => Err(app_error(format!(
            "unsupported image input type for {} (expected jpg, jpeg, png, gif, or webp)",
            path.display()
        ))),
    }
}

fn encode_base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut output = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0];
        let b1 = *chunk.get(1).unwrap_or(&0);
        let b2 = *chunk.get(2).unwrap_or(&0);
        output.push(ALPHABET[(b0 >> 2) as usize] as char);
        output.push(ALPHABET[(((b0 & 0b0000_0011) << 4) | (b1 >> 4)) as usize] as char);
        if chunk.len() > 1 {
            output.push(ALPHABET[(((b1 & 0b0000_1111) << 2) | (b2 >> 6)) as usize] as char);
        } else {
            output.push('=');
        }
        if chunk.len() > 2 {
            output.push(ALPHABET[(b2 & 0b0011_1111) as usize] as char);
        } else {
            output.push('=');
        }
    }
    output
}

fn run_task(
    config: AppConfig,
    task: String,
    skill: Option<String>,
    budget: Option<usize>,
    json: bool,
    image_inputs: Vec<ImageInput>,
    resumed_session: Option<String>,
) -> AppResult<()> {
    if task.trim().is_empty() {
        return Err(app_error("exec requires a non-empty prompt"));
    }

    if json {
        print_json_line(session_started_event(&task, resumed_session.as_deref()));
    }

    let runtime_store =
        RuntimeStore::new(PathBuf::from(&config.workspace.config_dir).join("runtime"));
    let rollback_store =
        RollbackStore::new(PathBuf::from(&config.workspace.config_dir).join("rollback"));
    let rollback_snapshot_id = create_exec_rollback_snapshot(&rollback_store, &task);
    let runtime_model = config.model.model.clone();
    let runtime_workspace = std::env::current_dir()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|_| ".".to_string());

    let context = TaskContext::with_image_inputs(task.clone(), skill, image_inputs);
    let agent = AgentLoop::new(config);
    let run_events = if json {
        Some(Rc::new(RefCell::new(JsonRunEvents)) as SharedAgentRunEvents)
    } else {
        None
    };
    let stream_events = if json {
        Some(Box::new(JsonStreamEvents) as Box<dyn StreamEvents>)
    } else {
        None
    };
    let options = AgentLoopOptions {
        steps: budget.unwrap_or_else(|| AgentLoopOptions::default().steps),
        emit_progress: !json,
        stream_events,
        run_events,
        ..AgentLoopOptions::default()
    };

    match agent.run_with(context, options) {
        Ok(result) => {
            let runtime_record = record_exec_runtime(
                &runtime_store,
                &task,
                &runtime_workspace,
                &runtime_model,
                &result,
            )?;
            if let Some(snapshot_id) = rollback_snapshot_id {
                let _ = rollback_store.bind_snapshot_runtime(
                    &snapshot_id,
                    Some(&runtime_record.thread_id),
                    Some(&runtime_record.assistant_turn_id),
                );
            }
            if json {
                print_json_line(assistant_final_event(&result));
            }
            Ok(())
        }
        Err(error) => {
            if json {
                print_json_line(error_event(&error.to_string()));
            }
            Err(error)
        }
    }
}

fn create_exec_rollback_snapshot(store: &RollbackStore, task: &str) -> Option<String> {
    let workspace = std::env::current_dir().ok()?;
    let label = format!("exec rollback: {}", runtime_thread_title(task));
    store
        .create_snapshot(&workspace, label)
        .ok()
        .map(|snapshot| snapshot.id)
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ExecRuntimeRecord {
    session_id: String,
    thread_id: String,
    user_turn_id: String,
    assistant_turn_id: String,
}

fn record_exec_runtime(
    store: &RuntimeStore,
    task: &str,
    workspace: &str,
    model: &str,
    result: &RunResult,
) -> AppResult<ExecRuntimeRecord> {
    let title = runtime_thread_title(task);
    let session = store.create_session(title.clone(), workspace.to_string())?;
    let thread = store.create_thread_for_session(
        &session.id,
        title,
        workspace.to_string(),
        model.to_string(),
        "agent".to_string(),
    )?;
    let user = store.append_turn(&thread.id, "user".to_string(), task.to_string())?;
    store.append_item(
        &thread.id,
        Some(&user.id),
        "message".to_string(),
        Some("user".to_string()),
        task.to_string(),
        "completed".to_string(),
    )?;
    let assistant = store.append_turn(
        &thread.id,
        "assistant".to_string(),
        result.final_message.clone(),
    )?;
    store.append_item(
        &thread.id,
        Some(&assistant.id),
        "message".to_string(),
        Some("assistant".to_string()),
        result.final_message.clone(),
        "completed".to_string(),
    )?;
    let usage_model = result.usage.model.as_deref().unwrap_or(model);
    store.append_usage_with_cache(
        &thread.id,
        Some(&assistant.id),
        usage_model.to_string(),
        "exec".to_string(),
        result.usage.prompt,
        result.usage.completion,
        result.usage.prompt_cache_hit,
        result.usage.prompt_cache_miss,
    )?;
    store.create_task(
        Some(&session.id),
        Some(&thread.id),
        None,
        "exec".to_string(),
        "completed".to_string(),
        result.final_message.clone(),
    )?;
    Ok(ExecRuntimeRecord {
        session_id: session.id,
        thread_id: thread.id,
        user_turn_id: user.id,
        assistant_turn_id: assistant.id,
    })
}

fn runtime_thread_title(task: &str) -> String {
    let title = task
        .lines()
        .find(|line| !line.trim().is_empty())
        .map(str::trim)
        .unwrap_or("Exec task");
    let clipped = title.chars().take(80).collect::<String>();
    if clipped.is_empty() {
        "Exec task".to_string()
    } else {
        clipped
    }
}

fn resolve_prompt_argument(prompt: &str) -> AppResult<String> {
    if prompt == "-" {
        read_stdin_prompt()
    } else {
        Ok(prompt.to_string())
    }
}

fn read_stdin_prompt() -> AppResult<String> {
    let mut input = String::new();
    io::stdin().read_to_string(&mut input)?;
    if input.trim().is_empty() {
        return Err(app_error("stdin prompt is empty"));
    }
    Ok(input)
}

fn build_resume_task(snapshot: &SessionSnapshot, followup: Option<&str>) -> String {
    match followup {
        Some(prompt) if !prompt.trim().is_empty() => format!(
            "Resume session {}.\nOriginal task:\n{}\n\nFollow-up task:\n{}",
            snapshot.id, snapshot.task, prompt
        ),
        _ => snapshot.task.clone(),
    }
}

fn print_json_line(value: JsonValue) {
    println!("{}", json_value_to_string(&value));
}

struct JsonStreamEvents;

impl StreamEvents for JsonStreamEvents {
    fn on_reasoning_delta(&mut self, chunk: &str) {
        if !chunk.is_empty() {
            print_json_line(assistant_reasoning_delta_event(chunk));
        }
    }

    fn on_text_delta(&mut self, chunk: &str) {
        if !chunk.is_empty() {
            print_json_line(assistant_delta_event(chunk));
        }
    }

    fn on_assistant_done(&mut self, _full_text: &str) {}

    fn on_tool_call(&mut self, _name: &str, _input: &BTreeMap<String, String>) {}
}

struct JsonRunEvents;

impl AgentRunEvents for JsonRunEvents {
    fn on_tool_call(&mut self, tool_name: &str, input: &BTreeMap<String, String>) {
        print_json_line(tool_call_parts_event(tool_name, input));
    }

    fn on_permission_request(
        &mut self,
        tool_name: &str,
        input: &BTreeMap<String, String>,
        kind: &str,
        target: &str,
    ) {
        print_json_line(permission_request_event(tool_name, input, kind, target));
    }

    fn on_tool_result(&mut self, event: &ToolEvent) {
        print_json_line(tool_result_event(event));
    }
}

fn session_started_event(task: &str, resumed_session: Option<&str>) -> JsonValue {
    let mut root = BTreeMap::new();
    root.insert(
        "type".to_string(),
        JsonValue::String("session_started".to_string()),
    );
    root.insert("task".to_string(), JsonValue::String(task.to_string()));
    root.insert(
        "resumed_session".to_string(),
        resumed_session
            .map(|value| JsonValue::String(value.to_string()))
            .unwrap_or(JsonValue::Null),
    );
    JsonValue::Object(root)
}

fn tool_call_parts_event(tool_name: &str, input: &BTreeMap<String, String>) -> JsonValue {
    let mut root = BTreeMap::new();
    root.insert(
        "type".to_string(),
        JsonValue::String("tool_call".to_string()),
    );
    root.insert("tool".to_string(), JsonValue::String(tool_name.to_string()));
    root.insert(
        "input".to_string(),
        JsonValue::Object(
            input
                .iter()
                .map(|(key, value)| (key.clone(), JsonValue::String(value.clone())))
                .collect(),
        ),
    );
    JsonValue::Object(root)
}

fn permission_request_event(
    tool_name: &str,
    input: &BTreeMap<String, String>,
    kind: &str,
    target: &str,
) -> JsonValue {
    let mut root = BTreeMap::new();
    root.insert(
        "type".to_string(),
        JsonValue::String("permission_request".to_string()),
    );
    root.insert("tool".to_string(), JsonValue::String(tool_name.to_string()));
    root.insert("kind".to_string(), JsonValue::String(kind.to_string()));
    root.insert("target".to_string(), JsonValue::String(target.to_string()));
    root.insert(
        "input".to_string(),
        JsonValue::Object(
            input
                .iter()
                .map(|(key, value)| (key.clone(), JsonValue::String(value.clone())))
                .collect(),
        ),
    );
    JsonValue::Object(root)
}

fn tool_result_event(event: &ToolEvent) -> JsonValue {
    let mut root = BTreeMap::new();
    root.insert(
        "type".to_string(),
        JsonValue::String("tool_result".to_string()),
    );
    root.insert(
        "tool".to_string(),
        JsonValue::String(event.tool_name.clone()),
    );
    root.insert(
        "status".to_string(),
        JsonValue::String(status_label(event.status).to_string()),
    );
    root.insert(
        "output".to_string(),
        JsonValue::String(event.output.clone()),
    );
    JsonValue::Object(root)
}

fn assistant_delta_event(message: &str) -> JsonValue {
    let mut root = BTreeMap::new();
    root.insert(
        "type".to_string(),
        JsonValue::String("assistant_delta".to_string()),
    );
    root.insert("delta".to_string(), JsonValue::String(message.to_string()));
    JsonValue::Object(root)
}

fn assistant_reasoning_delta_event(message: &str) -> JsonValue {
    let mut root = BTreeMap::new();
    root.insert(
        "type".to_string(),
        JsonValue::String("assistant_reasoning_delta".to_string()),
    );
    root.insert("delta".to_string(), JsonValue::String(message.to_string()));
    JsonValue::Object(root)
}

fn assistant_final_event(result: &RunResult) -> JsonValue {
    let failed_tool_calls = result
        .tool_events
        .iter()
        .filter(|event| matches!(event.status, ObservationStatus::Failed))
        .count();

    let mut usage = BTreeMap::new();
    usage.insert(
        "prompt".to_string(),
        JsonValue::Number(result.usage.prompt.to_string()),
    );
    usage.insert(
        "completion".to_string(),
        JsonValue::Number(result.usage.completion.to_string()),
    );
    usage.insert(
        "prompt_cache_hit".to_string(),
        JsonValue::Number(result.usage.prompt_cache_hit.to_string()),
    );
    usage.insert(
        "prompt_cache_miss".to_string(),
        JsonValue::Number(result.usage.prompt_cache_miss.to_string()),
    );

    let mut root = BTreeMap::new();
    root.insert(
        "type".to_string(),
        JsonValue::String("assistant_final".to_string()),
    );
    root.insert(
        "message".to_string(),
        JsonValue::String(result.final_message.clone()),
    );
    root.insert(
        "tool_calls".to_string(),
        JsonValue::Number(result.tool_events.len().to_string()),
    );
    root.insert(
        "failed_tool_calls".to_string(),
        JsonValue::Number(failed_tool_calls.to_string()),
    );
    root.insert("usage".to_string(), JsonValue::Object(usage));
    JsonValue::Object(root)
}

fn error_event(message: &str) -> JsonValue {
    let mut root = BTreeMap::new();
    root.insert("type".to_string(), JsonValue::String("error".to_string()));
    root.insert(
        "message".to_string(),
        JsonValue::String(message.to_string()),
    );
    JsonValue::Object(root)
}

fn status_label(status: ObservationStatus) -> &'static str {
    match status {
        ObservationStatus::Ok => "ok",
        ObservationStatus::Failed => "failed",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::protocol::TokenUsage;

    #[test]
    fn build_resume_task_combines_snapshot_and_followup() {
        let snapshot = SessionSnapshot {
            id: "session-123".to_string(),
            task: "Inspect the repository".to_string(),
            profile: "rust".to_string(),
        };

        let task = build_resume_task(&snapshot, Some("Apply the fix"));

        assert!(task.contains("Resume session session-123."));
        assert!(task.contains("Original task:\nInspect the repository"));
        assert!(task.contains("Follow-up task:\nApply the fix"));
    }

    #[test]
    fn build_resume_task_reuses_original_task_without_followup() {
        let snapshot = SessionSnapshot {
            id: "session-123".to_string(),
            task: "Inspect the repository".to_string(),
            profile: "rust".to_string(),
        };

        assert_eq!(build_resume_task(&snapshot, None), "Inspect the repository");
    }

    #[test]
    fn json_result_events_include_tool_and_final_records() {
        let mut input = BTreeMap::new();
        input.insert("path".to_string(), "src/main.rs".to_string());
        let result = RunResult {
            final_message: "done".to_string(),
            tool_events: vec![ToolEvent {
                tool_name: "read_file".to_string(),
                input,
                output: "1: fn main() {}".to_string(),
                status: ObservationStatus::Ok,
            }],
            usage: TokenUsage::new(12, 3),
        };

        let call = json_value_to_string(&tool_call_parts_event(
            &result.tool_events[0].tool_name,
            &result.tool_events[0].input,
        ));
        let tool_result = json_value_to_string(&tool_result_event(&result.tool_events[0]));
        let final_event = json_value_to_string(&assistant_final_event(&result));

        assert!(call.contains(r#""type":"tool_call""#));
        assert!(call.contains(r#""path":"src/main.rs""#));
        assert!(tool_result.contains(r#""status":"ok""#));
        assert!(final_event.contains(r#""type":"assistant_final""#));
        assert!(final_event.contains(r#""tool_calls":1"#));
        assert!(final_event.contains(r#""prompt":12"#));
    }

    #[test]
    fn record_exec_runtime_persists_thread_turns_and_usage() {
        let root = std::env::temp_dir().join(format!(
            "deepseek-exec-runtime-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let store = RuntimeStore::new(root);
        let mut usage = TokenUsage::new(12, 3);
        usage.model = Some("deepseek-v4-pro".to_string());
        let result = RunResult {
            final_message: "done".to_string(),
            tool_events: Vec::new(),
            usage,
        };

        let runtime_record = record_exec_runtime(
            &store,
            "Inspect the runtime\nwith details",
            ".",
            "deepseek-coder",
            &result,
        )
        .unwrap();

        let threads = store.list_threads(10).unwrap();
        assert_eq!(threads.len(), 1);
        assert_eq!(threads[0].title, "Inspect the runtime");
        let sessions = store.list_sessions(10).unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(
            sessions[0].active_thread_id.as_deref(),
            Some(threads[0].id.as_str())
        );
        assert_eq!(
            threads[0].session_id.as_deref(),
            Some(sessions[0].id.as_str())
        );
        assert_eq!(runtime_record.session_id, sessions[0].id);
        assert_eq!(runtime_record.thread_id, threads[0].id);
        let turns = store.list_turns(&threads[0].id).unwrap();
        assert_eq!(turns.len(), 2);
        assert_eq!(turns[0].role, "user");
        assert_eq!(turns[1].role, "assistant");
        assert_eq!(runtime_record.user_turn_id, turns[0].id);
        assert_eq!(runtime_record.assistant_turn_id, turns[1].id);
        let items = store.list_items(&threads[0].id, None).unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].turn_id.as_deref(), Some(turns[0].id.as_str()));
        assert_eq!(items[0].role.as_deref(), Some("user"));
        assert_eq!(items[1].turn_id.as_deref(), Some(turns[1].id.as_str()));
        assert_eq!(items[1].role.as_deref(), Some("assistant"));
        let usage = store.list_usage(Some(&threads[0].id), 10).unwrap();
        assert_eq!(usage.len(), 1);
        assert_eq!(usage[0].total_tokens, 15);
        assert_eq!(usage[0].model, "deepseek-v4-pro");
        let tasks = store
            .list_tasks(Some(&sessions[0].id), Some(&threads[0].id), 10)
            .unwrap();
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].kind, "exec");
        assert_eq!(tasks[0].status, "completed");
        let events = store.read_events(&threads[0].id, 0).unwrap();
        assert_eq!(events.len(), 7);
        assert_eq!(events[2].kind, "item_recorded");
        assert_eq!(events[4].kind, "item_recorded");
        assert_eq!(events[5].kind, "usage_recorded");
        assert_eq!(events[6].kind, "task_recorded");
    }

    #[test]
    fn permission_request_event_includes_kind_target_and_input() {
        let mut input = BTreeMap::new();
        input.insert("command".to_string(), "cargo test".to_string());

        let event = json_value_to_string(&permission_request_event(
            "run_shell",
            &input,
            "shell",
            "cargo test",
        ));

        assert!(event.contains(r#""type":"permission_request""#));
        assert!(event.contains(r#""tool":"run_shell""#));
        assert!(event.contains(r#""kind":"shell""#));
        assert!(event.contains(r#""target":"cargo test""#));
        assert!(event.contains(r#""command":"cargo test""#));
    }

    #[test]
    fn task_with_image_references_appends_existing_paths() {
        let path = std::env::temp_dir().join(format!(
            "deepseek-exec-image-{}-{}.png",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::write(&path, b"not a real png").unwrap();

        let images = load_image_inputs(&[path.display().to_string()]).unwrap();
        let task = task_with_image_references("Inspect this".to_string(), &images);

        assert!(task.contains("Inspect this"));
        assert!(task.contains("Attached image files:"));
        assert!(task.contains(&path.display().to_string()));
        assert!(task.contains("image/png"));

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn task_with_image_references_rejects_missing_path() {
        let error =
            load_image_inputs(&["/tmp/deepseek-missing-image-input.png".to_string()]).unwrap_err();

        assert!(error.to_string().contains("image input not found"));
    }

    #[test]
    fn image_inputs_reject_unsupported_extension() {
        let path = std::env::temp_dir().join(format!(
            "deepseek-exec-image-{}-{}.txt",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::write(&path, b"not an image").unwrap();

        let error = load_image_inputs(&[path.display().to_string()])
            .unwrap_err()
            .to_string();

        assert!(error.contains("unsupported image input type"));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn encode_base64_pads_short_chunks() {
        assert_eq!(encode_base64(b""), "");
        assert_eq!(encode_base64(b"f"), "Zg==");
        assert_eq!(encode_base64(b"fo"), "Zm8=");
        assert_eq!(encode_base64(b"foo"), "Zm9v");
    }
}
