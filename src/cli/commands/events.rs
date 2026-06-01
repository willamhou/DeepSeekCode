use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use crate::cli::app::{EventsAction, EventsDiffArgs, EventsReplayArgs};
use crate::config::load::load_or_default;
use crate::core::runtime::{json_array, json_object, RuntimeEvent, RuntimeStore, UsageRecord};
use crate::error::AppResult;
use crate::util::json::{
    json_as_array, json_as_object, json_as_string, json_as_u64, json_value_to_string,
    parse_json_value, JsonValue,
};

const DEFAULT_REPLAY_LIMIT: usize = 200;

#[derive(Debug, Clone, PartialEq, Eq)]
struct EventReplaySummary {
    thread_id: String,
    thread_title: String,
    total_event_count: usize,
    shown_event_count: usize,
    truncated: bool,
    events: Vec<EventLineSummary>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct EventLineSummary {
    seq: u64,
    kind: String,
    created_at: String,
    turn_id: Option<String>,
    label: String,
    payload_keys: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct EventsDiffSummary {
    left: ThreadEventMetrics,
    right: ThreadEventMetrics,
    kind_delta: BTreeMap<String, i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ThreadEventMetrics {
    thread_id: String,
    thread_title: String,
    event_count: u64,
    kind_counts: BTreeMap<String, u64>,
    model_turns: u64,
    estimated_total_cost_microusd: u64,
    unpriced_usage_records: u64,
    prompt_cache_hit_tokens: u64,
    prompt_cache_miss_tokens: u64,
    prompt_cache_hit_basis_points: u64,
    tool_call_count: u64,
    failed_tool_call_count: u64,
    file_mutating_tool_calls: u64,
    files_modified: Vec<String>,
    repair_count: u64,
    repeated_tool_suppressions: u64,
    prompt_layer_snapshot_count: u64,
}

pub fn run(action: EventsAction) -> AppResult<()> {
    let config = load_or_default()?;
    let store = RuntimeStore::new(PathBuf::from(&config.workspace.config_dir).join("runtime"));
    match action {
        EventsAction::Replay(args) => {
            let summary = events_replay_summary(&store, &args)?;
            if args.json {
                println!(
                    "{}",
                    json_value_to_string(&replay_summary_to_json(&summary))
                );
            } else {
                println!("{}", render_replay_summary(&summary));
            }
        }
        EventsAction::Diff(args) => {
            let summary = events_diff_summary(&store, &args)?;
            if args.json {
                println!("{}", json_value_to_string(&diff_summary_to_json(&summary)));
            } else {
                println!("{}", render_diff_summary(&summary));
            }
        }
    }
    Ok(())
}

fn events_replay_summary(
    store: &RuntimeStore,
    args: &EventsReplayArgs,
) -> AppResult<EventReplaySummary> {
    let thread = store.load_thread(&args.thread)?;
    let limit = args.limit.unwrap_or(DEFAULT_REPLAY_LIMIT);
    let events = store.read_events(&args.thread, 0)?;
    let total_event_count = events.len();
    let selected = events.into_iter().take(limit).collect::<Vec<_>>();
    let events = selected.iter().map(event_line_summary).collect::<Vec<_>>();
    Ok(EventReplaySummary {
        thread_id: thread.id,
        thread_title: thread.title,
        total_event_count,
        shown_event_count: events.len(),
        truncated: events.len() < total_event_count,
        events,
    })
}

fn event_line_summary(event: &RuntimeEvent) -> EventLineSummary {
    EventLineSummary {
        seq: event.seq,
        kind: event.kind.clone(),
        created_at: event.created_at.clone(),
        turn_id: event.turn_id.clone(),
        label: event_label(event),
        payload_keys: payload_keys(&event.payload),
    }
}

fn events_diff_summary(
    store: &RuntimeStore,
    args: &EventsDiffArgs,
) -> AppResult<EventsDiffSummary> {
    let left = thread_event_metrics(store, &args.left_thread)?;
    let right = thread_event_metrics(store, &args.right_thread)?;
    let mut kinds = BTreeSet::new();
    kinds.extend(left.kind_counts.keys().cloned());
    kinds.extend(right.kind_counts.keys().cloned());
    let kind_delta = kinds
        .into_iter()
        .map(|kind| {
            let left_count = left.kind_counts.get(&kind).copied().unwrap_or(0) as i64;
            let right_count = right.kind_counts.get(&kind).copied().unwrap_or(0) as i64;
            (kind, right_count - left_count)
        })
        .filter(|(_, delta)| *delta != 0)
        .collect::<BTreeMap<_, _>>();
    Ok(EventsDiffSummary {
        left,
        right,
        kind_delta,
    })
}

fn thread_event_metrics(store: &RuntimeStore, thread_id: &str) -> AppResult<ThreadEventMetrics> {
    let thread = store.load_thread(thread_id)?;
    let events = store.read_events(thread_id, 0)?;
    let usage = store.list_usage(Some(thread_id), usize::MAX)?;
    let items = store.list_items(thread_id, None)?;
    let mut metrics = ThreadEventMetrics {
        thread_id: thread.id.clone(),
        thread_title: thread.title,
        event_count: events.len() as u64,
        kind_counts: BTreeMap::new(),
        model_turns: usage.len() as u64,
        estimated_total_cost_microusd: 0,
        unpriced_usage_records: 0,
        prompt_cache_hit_tokens: 0,
        prompt_cache_miss_tokens: 0,
        prompt_cache_hit_basis_points: 0,
        tool_call_count: 0,
        failed_tool_call_count: 0,
        file_mutating_tool_calls: 0,
        files_modified: Vec::new(),
        repair_count: 0,
        repeated_tool_suppressions: 0,
        prompt_layer_snapshot_count: 0,
    };

    for event in &events {
        *metrics.kind_counts.entry(event.kind.clone()).or_insert(0) += 1;
        if event.kind == "tool_call_repair" {
            metrics.repair_count = metrics.repair_count.saturating_add(1);
        }
        if event.kind == "prompt_layers_recorded" {
            metrics.prompt_layer_snapshot_count = metrics
                .prompt_layer_snapshot_count
                .saturating_add(prompt_layer_snapshot_count(event));
        }
        if json_value_to_string(&event.payload).contains("repeated identical") {
            metrics.repeated_tool_suppressions =
                metrics.repeated_tool_suppressions.saturating_add(1);
        }
    }

    for record in &usage {
        accumulate_usage_metrics(&mut metrics, record);
    }
    let accounted = metrics
        .prompt_cache_hit_tokens
        .saturating_add(metrics.prompt_cache_miss_tokens);
    metrics.prompt_cache_hit_basis_points = if accounted == 0 {
        0
    } else {
        metrics.prompt_cache_hit_tokens.saturating_mul(10_000) / accounted
    };

    let mut modified_files = BTreeSet::new();
    let tool_call_items = items
        .iter()
        .filter(|item| item.item_type == "tool_call")
        .count() as u64;
    let tool_result_items = items
        .iter()
        .filter(|item| item.item_type == "tool_result")
        .count() as u64;
    metrics.tool_call_count = if tool_call_items > 0 {
        tool_call_items
    } else {
        tool_result_items
    };

    for item in &items {
        if item.item_type == "event" && item.content.contains("tool_call_repair") {
            metrics.repair_count = metrics.repair_count.saturating_add(1);
        }
        if item.content.contains("repeated identical")
            && (item.item_type == "tool_result" || item.item_type == "event")
        {
            metrics.repeated_tool_suppressions =
                metrics.repeated_tool_suppressions.saturating_add(1);
        }
        if item.item_type == "tool_result"
            && (item.status == "failed" || item.content.contains("status: failed"))
        {
            metrics.failed_tool_call_count = metrics.failed_tool_call_count.saturating_add(1);
        }
        if item.item_type == "tool_call" {
            let Some(tool_name) = tool_name_from_item_content(&item.content) else {
                continue;
            };
            if is_file_mutating_tool(&tool_name) {
                metrics.file_mutating_tool_calls =
                    metrics.file_mutating_tool_calls.saturating_add(1);
                for path in file_paths_from_tool_item(&item.content) {
                    modified_files.insert(path);
                }
            }
        }
    }
    metrics.files_modified = modified_files.into_iter().collect();
    Ok(metrics)
}

fn accumulate_usage_metrics(metrics: &mut ThreadEventMetrics, record: &UsageRecord) {
    metrics.prompt_cache_hit_tokens = metrics
        .prompt_cache_hit_tokens
        .saturating_add(record.prompt_cache_hit_tokens);
    metrics.prompt_cache_miss_tokens = metrics
        .prompt_cache_miss_tokens
        .saturating_add(record.prompt_cache_miss_tokens);
    if let Some(total) = record.estimated_total_cost_microusd {
        metrics.estimated_total_cost_microusd =
            metrics.estimated_total_cost_microusd.saturating_add(total);
    } else {
        metrics.unpriced_usage_records = metrics.unpriced_usage_records.saturating_add(1);
    }
}

fn prompt_layer_snapshot_count(event: &RuntimeEvent) -> u64 {
    json_as_object(&event.payload)
        .and_then(|root| root.get("snapshots"))
        .and_then(json_as_array)
        .map(|snapshots| snapshots.len() as u64)
        .unwrap_or(0)
}

fn event_label(event: &RuntimeEvent) -> String {
    let Some(root) = json_as_object(&event.payload) else {
        return "payload: non-object".to_string();
    };
    match event.kind.as_str() {
        "thread_created" => {
            let title = root.get("title").and_then(json_as_string).unwrap_or("-");
            let mode = root.get("mode").and_then(json_as_string).unwrap_or("-");
            format!("title={title}; mode={mode}")
        }
        "turn_recorded" | "turn_updated" => {
            let role = root.get("role").and_then(json_as_string).unwrap_or("-");
            let status = root.get("status").and_then(json_as_string).unwrap_or("-");
            let turn_id = root.get("turn_id").and_then(json_as_string).unwrap_or("-");
            format!("turn={turn_id}; role={role}; status={status}")
        }
        "item_recorded" | "item_updated" => {
            let item_type = root
                .get("item_type")
                .and_then(json_as_string)
                .unwrap_or("-");
            let status = root.get("status").and_then(json_as_string).unwrap_or("-");
            format!("item_type={item_type}; status={status}")
        }
        "usage_recorded" => {
            let model = root.get("model").and_then(json_as_string).unwrap_or("-");
            let total = root.get("total_tokens").and_then(json_as_u64).unwrap_or(0);
            let hit = root
                .get("prompt_cache_hit_tokens")
                .and_then(json_as_u64)
                .unwrap_or(0);
            let miss = root
                .get("prompt_cache_miss_tokens")
                .and_then(json_as_u64)
                .unwrap_or(0);
            format!("model={model}; total_tokens={total}; cache_hit={hit}; cache_miss={miss}")
        }
        "prompt_layers_recorded" => {
            let digest = root.get("digest").and_then(json_as_string).unwrap_or("-");
            let snapshots = root
                .get("snapshots")
                .and_then(json_as_array)
                .map(|items| items.len())
                .unwrap_or(0);
            format!("digest={digest}; snapshots={snapshots}")
        }
        "tool_call_repair" => {
            let kind = root.get("kind").and_then(json_as_string).unwrap_or("-");
            let tool = root
                .get("tool_name")
                .and_then(json_as_string)
                .unwrap_or("-");
            let detail = root.get("detail").and_then(json_as_string).unwrap_or("-");
            format!(
                "tool={tool}; repair={kind}; detail={}",
                compact_line(detail, 100)
            )
        }
        "permission_request" => {
            let tool = root.get("tool").and_then(json_as_string).unwrap_or("-");
            let kind = root.get("kind").and_then(json_as_string).unwrap_or("-");
            let target = root.get("target").and_then(json_as_string).unwrap_or("-");
            format!(
                "tool={tool}; approval={kind}; target={}",
                compact_line(target, 100)
            )
        }
        "thread_goal_set" => {
            let objective = root
                .get("objective")
                .and_then(json_as_string)
                .unwrap_or("-");
            format!("objective={}", compact_line(objective, 120))
        }
        "thread_goal_cleared" => "goal cleared".to_string(),
        "thread_budget_set" => {
            let budget = root
                .get("session_budget_microusd")
                .and_then(json_as_u64)
                .map(|value| value.to_string())
                .unwrap_or_else(|| "off".to_string());
            format!("session_budget_microusd={budget}")
        }
        "task_recorded" | "task_updated" | "task_claimed" => {
            let kind = root.get("kind").and_then(json_as_string).unwrap_or("-");
            let status = root.get("status").and_then(json_as_string).unwrap_or("-");
            let summary = root.get("summary").and_then(json_as_string).unwrap_or("-");
            format!(
                "task={kind}; status={status}; summary={}",
                compact_line(summary, 100)
            )
        }
        other => {
            let keys = payload_keys(&event.payload);
            if keys.is_empty() {
                other.to_string()
            } else {
                format!("payload keys: {}", keys.join(", "))
            }
        }
    }
}

fn payload_keys(payload: &JsonValue) -> Vec<String> {
    json_as_object(payload)
        .map(|root| root.keys().cloned().collect::<Vec<_>>())
        .unwrap_or_default()
}

fn render_replay_summary(summary: &EventReplaySummary) -> String {
    let mut out = String::new();
    out.push_str("DeepSeekCode events replay\n");
    out.push_str(&format!(
        "thread: {} ({})\n",
        summary.thread_id, summary.thread_title
    ));
    out.push_str(&format!(
        "events: {} shown / {} total\n",
        summary.shown_event_count, summary.total_event_count
    ));
    if summary.truncated {
        out.push_str("truncated: true\n");
    }
    for event in &summary.events {
        let turn = event
            .turn_id
            .as_deref()
            .map(|value| format!(" turn={value}"))
            .unwrap_or_default();
        out.push_str(&format!(
            "#{} {}{} @ {} - {}\n",
            event.seq, event.kind, turn, event.created_at, event.label
        ));
    }
    out.trim_end().to_string()
}

fn render_diff_summary(summary: &EventsDiffSummary) -> String {
    let left = &summary.left;
    let right = &summary.right;
    let mut out = String::new();
    out.push_str("DeepSeekCode events diff\n");
    out.push_str(&format!(
        "left: {} ({})\n",
        left.thread_id, left.thread_title
    ));
    out.push_str(&format!(
        "right: {} ({})\n",
        right.thread_id, right.thread_title
    ));
    out.push_str(&format!(
        "event_count_delta: {} ({} -> {})\n",
        signed_delta(right.event_count, left.event_count),
        left.event_count,
        right.event_count
    ));
    out.push_str(&format!(
        "total_cost_delta_usd: {} ({} -> {})\n",
        microusd_signed_decimal(delta_i64(
            right.estimated_total_cost_microusd,
            left.estimated_total_cost_microusd,
        )),
        microusd_decimal(left.estimated_total_cost_microusd),
        microusd_decimal(right.estimated_total_cost_microusd)
    ));
    out.push_str(&format!(
        "cache_hit_rate_delta: {}pp ({} -> {})\n",
        basis_points_signed_decimal(delta_i64(
            right.prompt_cache_hit_basis_points,
            left.prompt_cache_hit_basis_points,
        )),
        basis_points_percent(left.prompt_cache_hit_basis_points),
        basis_points_percent(right.prompt_cache_hit_basis_points)
    ));
    out.push_str(&format!(
        "tool_call_delta: {} ({} -> {})\n",
        signed_delta(right.tool_call_count, left.tool_call_count),
        left.tool_call_count,
        right.tool_call_count
    ));
    out.push_str(&format!(
        "failed_tool_call_delta: {} ({} -> {})\n",
        signed_delta(right.failed_tool_call_count, left.failed_tool_call_count),
        left.failed_tool_call_count,
        right.failed_tool_call_count
    ));
    match files_modified_delta(left, right) {
        Some(delta) => out.push_str(&format!(
            "files_modified_delta: {} ({} -> {})\n",
            signed_i64(delta),
            left.files_modified.len(),
            right.files_modified.len()
        )),
        None => out.push_str(&format!(
            "files_modified_delta: unavailable (file_mutating_tool_delta: {})\n",
            signed_delta(
                right.file_mutating_tool_calls,
                left.file_mutating_tool_calls
            )
        )),
    }
    out.push_str(&format!(
        "repair_count_delta: {} ({} -> {})\n",
        signed_delta(right.repair_count, left.repair_count),
        left.repair_count,
        right.repair_count
    ));
    out.push_str(&format!(
        "repeated_tool_suppression_delta: {} ({} -> {})\n",
        signed_delta(
            right.repeated_tool_suppressions,
            left.repeated_tool_suppressions
        ),
        left.repeated_tool_suppressions,
        right.repeated_tool_suppressions
    ));
    if !summary.kind_delta.is_empty() {
        out.push_str("event_kind_delta:\n");
        for (kind, delta) in &summary.kind_delta {
            out.push_str(&format!("- {kind}: {}\n", signed_i64(*delta)));
        }
    }
    out.trim_end().to_string()
}

fn replay_summary_to_json(summary: &EventReplaySummary) -> JsonValue {
    json_object([
        (
            "schema",
            JsonValue::String("deepseek.events.replay.v1".to_string()),
        ),
        ("thread_id", JsonValue::String(summary.thread_id.clone())),
        (
            "thread_title",
            JsonValue::String(summary.thread_title.clone()),
        ),
        (
            "total_event_count",
            JsonValue::Number(summary.total_event_count.to_string()),
        ),
        (
            "shown_event_count",
            JsonValue::Number(summary.shown_event_count.to_string()),
        ),
        ("truncated", JsonValue::Bool(summary.truncated)),
        (
            "events",
            json_array(
                summary
                    .events
                    .iter()
                    .map(event_line_summary_to_json)
                    .collect::<Vec<_>>(),
            ),
        ),
    ])
}

fn event_line_summary_to_json(event: &EventLineSummary) -> JsonValue {
    let turn_id = event
        .turn_id
        .as_ref()
        .map(|value| JsonValue::String(value.clone()))
        .unwrap_or(JsonValue::Null);
    json_object([
        ("seq", JsonValue::Number(event.seq.to_string())),
        ("kind", JsonValue::String(event.kind.clone())),
        ("created_at", JsonValue::String(event.created_at.clone())),
        ("turn_id", turn_id),
        ("label", JsonValue::String(event.label.clone())),
        (
            "payload_keys",
            json_array(
                event
                    .payload_keys
                    .iter()
                    .map(|key| JsonValue::String(key.clone()))
                    .collect::<Vec<_>>(),
            ),
        ),
    ])
}

fn diff_summary_to_json(summary: &EventsDiffSummary) -> JsonValue {
    let files_modified_delta = files_modified_delta(&summary.left, &summary.right)
        .map(|delta| JsonValue::Number(delta.to_string()))
        .unwrap_or(JsonValue::Null);
    json_object([
        (
            "schema",
            JsonValue::String("deepseek.events.diff.v1".to_string()),
        ),
        ("left", metrics_to_json(&summary.left)),
        ("right", metrics_to_json(&summary.right)),
        (
            "event_count_delta",
            JsonValue::Number(
                delta_i64(summary.right.event_count, summary.left.event_count).to_string(),
            ),
        ),
        (
            "estimated_total_cost_microusd_delta",
            JsonValue::Number(
                delta_i64(
                    summary.right.estimated_total_cost_microusd,
                    summary.left.estimated_total_cost_microusd,
                )
                .to_string(),
            ),
        ),
        (
            "prompt_cache_hit_basis_points_delta",
            JsonValue::Number(
                delta_i64(
                    summary.right.prompt_cache_hit_basis_points,
                    summary.left.prompt_cache_hit_basis_points,
                )
                .to_string(),
            ),
        ),
        (
            "tool_call_count_delta",
            JsonValue::Number(
                delta_i64(summary.right.tool_call_count, summary.left.tool_call_count).to_string(),
            ),
        ),
        (
            "failed_tool_call_count_delta",
            JsonValue::Number(
                delta_i64(
                    summary.right.failed_tool_call_count,
                    summary.left.failed_tool_call_count,
                )
                .to_string(),
            ),
        ),
        ("files_modified_delta", files_modified_delta),
        (
            "file_mutating_tool_call_delta",
            JsonValue::Number(
                delta_i64(
                    summary.right.file_mutating_tool_calls,
                    summary.left.file_mutating_tool_calls,
                )
                .to_string(),
            ),
        ),
        (
            "repair_count_delta",
            JsonValue::Number(
                delta_i64(summary.right.repair_count, summary.left.repair_count).to_string(),
            ),
        ),
        (
            "repeated_tool_suppression_delta",
            JsonValue::Number(
                delta_i64(
                    summary.right.repeated_tool_suppressions,
                    summary.left.repeated_tool_suppressions,
                )
                .to_string(),
            ),
        ),
        (
            "event_kind_delta",
            JsonValue::Object(
                summary
                    .kind_delta
                    .iter()
                    .map(|(kind, delta)| (kind.clone(), JsonValue::Number(delta.to_string())))
                    .collect(),
            ),
        ),
    ])
}

fn metrics_to_json(metrics: &ThreadEventMetrics) -> JsonValue {
    json_object([
        ("thread_id", JsonValue::String(metrics.thread_id.clone())),
        (
            "thread_title",
            JsonValue::String(metrics.thread_title.clone()),
        ),
        (
            "event_count",
            JsonValue::Number(metrics.event_count.to_string()),
        ),
        (
            "model_turns",
            JsonValue::Number(metrics.model_turns.to_string()),
        ),
        (
            "estimated_total_cost_microusd",
            JsonValue::Number(metrics.estimated_total_cost_microusd.to_string()),
        ),
        (
            "unpriced_usage_records",
            JsonValue::Number(metrics.unpriced_usage_records.to_string()),
        ),
        (
            "prompt_cache_hit_tokens",
            JsonValue::Number(metrics.prompt_cache_hit_tokens.to_string()),
        ),
        (
            "prompt_cache_miss_tokens",
            JsonValue::Number(metrics.prompt_cache_miss_tokens.to_string()),
        ),
        (
            "prompt_cache_hit_basis_points",
            JsonValue::Number(metrics.prompt_cache_hit_basis_points.to_string()),
        ),
        (
            "tool_call_count",
            JsonValue::Number(metrics.tool_call_count.to_string()),
        ),
        (
            "failed_tool_call_count",
            JsonValue::Number(metrics.failed_tool_call_count.to_string()),
        ),
        (
            "file_mutating_tool_calls",
            JsonValue::Number(metrics.file_mutating_tool_calls.to_string()),
        ),
        (
            "files_modified",
            json_array(
                metrics
                    .files_modified
                    .iter()
                    .map(|path| JsonValue::String(path.clone()))
                    .collect::<Vec<_>>(),
            ),
        ),
        (
            "repair_count",
            JsonValue::Number(metrics.repair_count.to_string()),
        ),
        (
            "repeated_tool_suppressions",
            JsonValue::Number(metrics.repeated_tool_suppressions.to_string()),
        ),
        (
            "prompt_layer_snapshot_count",
            JsonValue::Number(metrics.prompt_layer_snapshot_count.to_string()),
        ),
        (
            "event_kind_counts",
            JsonValue::Object(
                metrics
                    .kind_counts
                    .iter()
                    .map(|(kind, count)| (kind.clone(), JsonValue::Number(count.to_string())))
                    .collect(),
            ),
        ),
    ])
}

fn files_modified_delta(left: &ThreadEventMetrics, right: &ThreadEventMetrics) -> Option<i64> {
    if left.files_modified.is_empty() && right.files_modified.is_empty() {
        None
    } else {
        Some(right.files_modified.len() as i64 - left.files_modified.len() as i64)
    }
}

fn tool_name_from_item_content(content: &str) -> Option<String> {
    content
        .lines()
        .find_map(|line| {
            line.trim()
                .strip_prefix("tool:")
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
        })
        .or_else(|| tool_name_from_json_item_content(content))
}

fn tool_name_from_json_item_content(content: &str) -> Option<String> {
    let parsed = parse_json_value(content.trim()).ok()?;
    let root = json_as_object(&parsed)?;
    root.get("tool")
        .or_else(|| root.get("name"))
        .and_then(json_as_string)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn is_file_mutating_tool(tool_name: &str) -> bool {
    matches!(
        tool_name,
        "apply_patch"
            | "write_file"
            | "edit_file"
            | "fim_edit"
            | "delete_file"
            | "copy_file"
            | "move_file"
            | "pandoc_convert"
            | "revert_turn"
    )
}

fn file_paths_from_tool_item(content: &str) -> Vec<String> {
    let tool_name = tool_name_from_item_content(content);
    let mut paths = file_paths_from_json_item_content(content, tool_name.as_deref());
    if !paths.is_empty() {
        return paths;
    }
    for line in content.lines() {
        let trimmed = line.trim();
        if let Some(target) = trimmed.strip_prefix("target:") {
            let target = target.trim();
            if !target.is_empty() {
                push_unique_path(&mut paths, target);
            }
        }
        if let Some(input) = trimmed.strip_prefix("input:") {
            push_paths_from_input_fields(&mut paths, tool_name.as_deref(), input);
        }
    }
    paths
}

fn file_paths_from_json_item_content(content: &str, tool_name: Option<&str>) -> Vec<String> {
    let Some(parsed) = parse_json_value(content.trim()).ok() else {
        return Vec::new();
    };
    let Some(root) = json_as_object(&parsed) else {
        return Vec::new();
    };
    let tool_name = tool_name
        .or_else(|| {
            root.get("tool")
                .or_else(|| root.get("name"))
                .and_then(json_as_string)
        })
        .unwrap_or("");
    let Some(arguments) = root.get("arguments").and_then(json_as_object) else {
        return Vec::new();
    };
    let mut paths = Vec::new();
    push_paths_from_json_arguments(&mut paths, tool_name, arguments);
    paths
}

fn push_paths_from_json_arguments(
    paths: &mut Vec<String>,
    tool_name: &str,
    arguments: &BTreeMap<String, JsonValue>,
) {
    match tool_name {
        "copy_file" => push_json_path(paths, arguments, "destination_path"),
        "move_file" => {
            push_json_path(paths, arguments, "source_path");
            push_json_path(paths, arguments, "destination_path");
        }
        "pandoc_convert" => push_json_path(paths, arguments, "output_path"),
        _ => {
            for key in [
                "path",
                "target",
                "file",
                "output_path",
                "destination_path",
                "source_path",
            ] {
                push_json_path(paths, arguments, key);
            }
        }
    }
}

fn push_json_path(paths: &mut Vec<String>, arguments: &BTreeMap<String, JsonValue>, key: &str) {
    if let Some(value) = arguments.get(key).and_then(json_as_string) {
        push_unique_path(paths, value);
    }
}

fn push_paths_from_input_fields(paths: &mut Vec<String>, tool_name: Option<&str>, input: &str) {
    let mut fields = BTreeMap::new();
    for field in input.split(", ") {
        let Some((key, value)) = field.split_once('=') else {
            continue;
        };
        fields.insert(key.trim().to_string(), value.trim().to_string());
    }
    match tool_name.unwrap_or("") {
        "copy_file" => push_text_path(paths, &fields, "destination_path"),
        "move_file" => {
            push_text_path(paths, &fields, "source_path");
            push_text_path(paths, &fields, "destination_path");
        }
        "pandoc_convert" => push_text_path(paths, &fields, "output_path"),
        _ => {
            for key in [
                "path",
                "target",
                "file",
                "output_path",
                "destination_path",
                "source_path",
            ] {
                push_text_path(paths, &fields, key);
            }
        }
    }
}

fn push_text_path(paths: &mut Vec<String>, fields: &BTreeMap<String, String>, key: &str) {
    if let Some(value) = fields.get(key) {
        push_unique_path(paths, value);
    }
}

fn push_unique_path(paths: &mut Vec<String>, value: &str) {
    let value = value.trim();
    if value.is_empty() || value == "{}" || paths.iter().any(|path| path == value) {
        return;
    }
    paths.push(value.to_string());
}

fn delta_i64(right: u64, left: u64) -> i64 {
    right as i64 - left as i64
}

fn signed_delta(right: u64, left: u64) -> String {
    signed_i64(delta_i64(right, left))
}

fn signed_i64(value: i64) -> String {
    if value >= 0 {
        format!("+{value}")
    } else {
        value.to_string()
    }
}

fn basis_points_percent(value: u64) -> String {
    format!("{}.{:02}%", value / 100, value % 100)
}

fn basis_points_signed_decimal(value: i64) -> String {
    let sign = if value >= 0 { "+" } else { "-" };
    let abs = value.unsigned_abs();
    format!("{sign}{}.{:02}", abs / 100, abs % 100)
}

fn microusd_decimal(value: u64) -> String {
    format!("{}.{:06}", value / 1_000_000, value % 1_000_000)
}

fn microusd_signed_decimal(value: i64) -> String {
    let sign = if value >= 0 { "+" } else { "-" };
    let abs = value.unsigned_abs();
    format!("{sign}{}.{:06}", abs / 1_000_000, abs % 1_000_000)
}

fn compact_line(value: &str, max_chars: usize) -> String {
    let line = value.lines().next().unwrap_or("").trim();
    if line.chars().count() <= max_chars {
        line.to_string()
    } else {
        format!("{}...", line.chars().take(max_chars).collect::<String>())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::runtime::{RuntimeStore, ThreadRecord};

    fn temp_store(label: &str) -> RuntimeStore {
        RuntimeStore::new(std::env::temp_dir().join(format!(
            "deepseek-events-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        )))
    }

    fn thread(store: &RuntimeStore, title: &str) -> ThreadRecord {
        let session = store
            .create_session(title.to_string(), ".".to_string())
            .unwrap();
        store
            .create_thread_for_session(
                &session.id,
                title.to_string(),
                ".".to_string(),
                "deepseek-v4-flash".to_string(),
                "agent".to_string(),
            )
            .unwrap()
    }

    #[test]
    fn replay_summary_renders_compact_runtime_event_labels() {
        let store = temp_store("replay");
        let thread = thread(&store, "Replay");
        let turn = store
            .append_turn(&thread.id, "assistant".to_string(), "done".to_string())
            .unwrap();
        let usage = store
            .append_usage_with_cache(
                &thread.id,
                Some(&turn.id),
                "deepseek-v4-flash".to_string(),
                "test".to_string(),
                100,
                25,
                80,
                20,
            )
            .unwrap();
        store
            .append_thread_event(
                &thread.id,
                "prompt_layers_recorded",
                json_object([
                    ("digest", JsonValue::String("digest-1".to_string())),
                    ("usage_id", JsonValue::String(usage.id)),
                    (
                        "snapshots",
                        json_array(vec![json_object([(
                            "estimated_tokens",
                            JsonValue::Number("42".to_string()),
                        )])]),
                    ),
                ]),
            )
            .unwrap();
        store
            .append_thread_event(
                &thread.id,
                "tool_call_repair",
                json_object([
                    ("kind", JsonValue::String("truncated-json".to_string())),
                    (
                        "detail",
                        JsonValue::String("repaired truncated tool arguments JSON".to_string()),
                    ),
                    ("tool_name", JsonValue::String("read_file".to_string())),
                ]),
            )
            .unwrap();

        let summary = events_replay_summary(
            &store,
            &EventsReplayArgs {
                thread: thread.id,
                limit: Some(20),
                json: false,
            },
        )
        .unwrap();
        let rendered = render_replay_summary(&summary);
        assert!(rendered.contains("DeepSeekCode events replay"));
        assert!(rendered.contains("usage_recorded"));
        assert!(rendered.contains("model=deepseek-v4-flash"));
        assert!(rendered.contains("prompt_layers_recorded"));
        assert!(rendered.contains("digest=digest-1"));
        assert!(rendered.contains("tool_call_repair"));
        assert!(rendered.contains("tool=read_file"));
        assert!(rendered.contains("repair=truncated-json"));
    }

    #[test]
    fn diff_summary_compares_cost_cache_tools_failures_and_files() {
        let store = temp_store("diff");
        let left = thread(&store, "Left");
        let right = thread(&store, "Right");
        let left_turn = store
            .append_turn(&left.id, "assistant".to_string(), "left".to_string())
            .unwrap();
        let right_turn = store
            .append_turn(&right.id, "assistant".to_string(), "right".to_string())
            .unwrap();
        store
            .append_usage_with_cache(
                &left.id,
                Some(&left_turn.id),
                "deepseek-v4-flash".to_string(),
                "test".to_string(),
                100,
                10,
                50,
                50,
            )
            .unwrap();
        store
            .append_usage_with_cache(
                &right.id,
                Some(&right_turn.id),
                "deepseek-v4-pro".to_string(),
                "test".to_string(),
                200,
                20,
                150,
                50,
            )
            .unwrap();
        store
            .append_item(
                &left.id,
                Some(&left_turn.id),
                "tool_call".to_string(),
                Some("tool".to_string()),
                "tool: read_file\nstatus: completed\ninput: path=src/lib.rs".to_string(),
                "completed".to_string(),
            )
            .unwrap();
        store
            .append_item(
                &right.id,
                Some(&right_turn.id),
                "tool_call".to_string(),
                Some("tool".to_string()),
                "tool: write_file\ntarget: src/main.rs\nstatus: completed\ninput: path=src/main.rs"
                    .to_string(),
                "completed".to_string(),
            )
            .unwrap();
        store
            .append_item(
                &right.id,
                Some(&right_turn.id),
                "tool_result".to_string(),
                Some("tool".to_string()),
                "tool: write_file\nstatus: failed\npermission denied".to_string(),
                "failed".to_string(),
            )
            .unwrap();
        store
            .append_thread_event(
                &right.id,
                "tool_call_repair",
                json_object([("kind", JsonValue::String("truncated-json".to_string()))]),
            )
            .unwrap();

        let summary = events_diff_summary(
            &store,
            &EventsDiffArgs {
                left_thread: left.id,
                right_thread: right.id,
                json: false,
            },
        )
        .unwrap();
        assert_eq!(summary.left.tool_call_count, 1);
        assert_eq!(summary.right.tool_call_count, 1);
        assert_eq!(summary.right.failed_tool_call_count, 1);
        assert_eq!(
            summary.right.files_modified,
            vec!["src/main.rs".to_string()]
        );
        assert_eq!(summary.right.repair_count, 1);
        assert!(
            summary.right.estimated_total_cost_microusd
                > summary.left.estimated_total_cost_microusd
        );
        let rendered = render_diff_summary(&summary);
        assert!(rendered.contains("failed_tool_call_delta: +1"));
        assert!(rendered.contains("files_modified_delta: +1"));
        assert!(rendered.contains("repair_count_delta: +1"));
    }

    #[test]
    fn diff_summary_extracts_structured_acp_file_write_paths() {
        let store = temp_store("diff-acp-json-files");
        let left = thread(&store, "Left");
        let right = thread(&store, "Right");
        let right_turn = store
            .append_turn(&right.id, "assistant".to_string(), "right".to_string())
            .unwrap();

        store
            .append_item(
                &right.id,
                Some(&right_turn.id),
                "tool_call".to_string(),
                Some("assistant".to_string()),
                json_value_to_string(&json_object([
                    ("tool", JsonValue::String("write_file".to_string())),
                    (
                        "arguments",
                        json_object([
                            ("path", JsonValue::String("src/acp.rs".to_string())),
                            ("content", JsonValue::String("fn main() {}\n".to_string())),
                        ]),
                    ),
                ])),
                "completed".to_string(),
            )
            .unwrap();

        let summary = events_diff_summary(
            &store,
            &EventsDiffArgs {
                left_thread: left.id,
                right_thread: right.id,
                json: true,
            },
        )
        .unwrap();
        assert_eq!(summary.right.file_mutating_tool_calls, 1);
        assert_eq!(summary.right.files_modified, vec!["src/acp.rs"]);
        let root = diff_summary_to_json(&summary);
        let JsonValue::Object(root) = root else {
            panic!("diff summary should be an object");
        };
        assert!(matches!(
            root.get("files_modified_delta"),
            Some(JsonValue::Number(value)) if value == "1"
        ));
    }

    #[test]
    fn diff_summary_tracks_mcp_copy_move_delete_targets() {
        let store = temp_store("diff-mcp-file-targets");
        let left = thread(&store, "Left");
        let right = thread(&store, "Right");
        let right_turn = store
            .append_turn(&right.id, "assistant".to_string(), "right".to_string())
            .unwrap();

        for (tool, arguments) in [
            (
                "copy_file",
                json_object([
                    (
                        "source_path",
                        JsonValue::String("src/source.rs".to_string()),
                    ),
                    (
                        "destination_path",
                        JsonValue::String("src/copied.rs".to_string()),
                    ),
                ]),
            ),
            (
                "move_file",
                json_object([
                    ("source_path", JsonValue::String("src/old.rs".to_string())),
                    (
                        "destination_path",
                        JsonValue::String("src/new.rs".to_string()),
                    ),
                ]),
            ),
            (
                "delete_file",
                json_object([("path", JsonValue::String("src/delete.rs".to_string()))]),
            ),
            (
                "pandoc_convert",
                json_object([
                    (
                        "source_path",
                        JsonValue::String("docs/source.md".to_string()),
                    ),
                    ("target_format", JsonValue::String("html".to_string())),
                    (
                        "output_path",
                        JsonValue::String("docs/source.html".to_string()),
                    ),
                ]),
            ),
        ] {
            store
                .append_item(
                    &right.id,
                    Some(&right_turn.id),
                    "tool_call".to_string(),
                    Some("assistant".to_string()),
                    json_value_to_string(&json_object([
                        ("tool", JsonValue::String(tool.to_string())),
                        ("arguments", arguments),
                    ])),
                    "completed".to_string(),
                )
                .unwrap();
        }

        let summary = events_diff_summary(
            &store,
            &EventsDiffArgs {
                left_thread: left.id,
                right_thread: right.id,
                json: false,
            },
        )
        .unwrap();
        assert_eq!(summary.right.file_mutating_tool_calls, 4);
        assert_eq!(
            summary.right.files_modified,
            vec![
                "docs/source.html".to_string(),
                "src/copied.rs".to_string(),
                "src/delete.rs".to_string(),
                "src/new.rs".to_string(),
                "src/old.rs".to_string(),
            ]
        );
        assert!(render_diff_summary(&summary).contains("files_modified_delta: +5"));
    }

    #[test]
    fn diff_json_uses_null_when_file_paths_are_unavailable() {
        let store = temp_store("diff-no-files");
        let left = thread(&store, "Left");
        let right = thread(&store, "Right");

        let summary = events_diff_summary(
            &store,
            &EventsDiffArgs {
                left_thread: left.id,
                right_thread: right.id,
                json: true,
            },
        )
        .unwrap();
        let root = diff_summary_to_json(&summary);
        let JsonValue::Object(root) = root else {
            panic!("diff summary should be an object");
        };
        assert!(matches!(
            root.get("files_modified_delta"),
            Some(JsonValue::Null)
        ));
    }
}
