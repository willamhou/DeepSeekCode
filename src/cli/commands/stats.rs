use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use crate::cli::app::StatsArgs;
use crate::config::load::load_or_default;
use crate::core::runtime::{RuntimeEvent, RuntimeStore, ThreadRecord, UsageRecord};
use crate::error::{app_error, AppResult};
use crate::util::json::{
    json_as_array, json_as_object, json_as_string, json_value_to_string, JsonValue,
};

const DEFAULT_LIMIT: usize = 500;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct StatsSummary {
    scope: String,
    thread_count: usize,
    turn_count: usize,
    model_turns: usize,
    prompt_tokens: u64,
    completion_tokens: u64,
    total_tokens: u64,
    prompt_cache_hit_tokens: u64,
    prompt_cache_miss_tokens: u64,
    prompt_cache_hit_basis_points: u64,
    estimated_input_cost_microusd: u64,
    estimated_output_cost_microusd: u64,
    estimated_total_cost_microusd: u64,
    unpriced_record_count: u64,
    model_counts: BTreeMap<String, u64>,
    model_preset_counts: BTreeMap<String, u64>,
    model_route_counts: BTreeMap<ModelRouteKey, u64>,
    repair_count: u64,
    repeated_tool_suppressions: u64,
    prompt_layer_snapshot_count: u64,
    latest_prompt_layer_estimated_tokens: u64,
    latest_prompt_layer_digest: Option<String>,
    prompt_layer_cache_stable_hash_changes: u64,
    prompt_layer_trends: BTreeMap<String, PromptLayerTrend>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct ModelRouteKey {
    preset: String,
    model: String,
    escalated: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct PromptLayerTrend {
    cache_stable: bool,
    snapshot_count: u64,
    first_estimated_tokens: u64,
    latest_estimated_tokens: u64,
    max_estimated_tokens: u64,
    hash_changes: u64,
    previous_hash: Option<String>,
    latest_hash: Option<String>,
}

pub fn run(args: StatsArgs) -> AppResult<()> {
    let config = load_or_default()?;
    let store = RuntimeStore::new(PathBuf::from(&config.workspace.config_dir).join("runtime"));
    let summary = stats_summary(&store, &args)?;
    if args.json {
        println!("{}", json_value_to_string(&stats_summary_to_json(&summary)));
    } else {
        println!("{}", render_stats_summary(&summary));
    }
    if args.require_prefix_stable {
        require_prompt_prefix_stable(&summary)?;
    }
    Ok(())
}

fn stats_summary(store: &RuntimeStore, args: &StatsArgs) -> AppResult<StatsSummary> {
    let limit = args.limit.unwrap_or(DEFAULT_LIMIT);
    let threads = selected_threads(store, args)?;
    let thread_ids = threads
        .iter()
        .map(|thread| thread.id.clone())
        .collect::<BTreeSet<_>>();
    let usage = selected_usage(store, args, &thread_ids, limit)?;
    let mut summary = StatsSummary {
        scope: stats_scope(args),
        thread_count: thread_ids.len(),
        model_turns: usage.len(),
        ..StatsSummary::default()
    };

    for record in &usage {
        accumulate_usage(&mut summary, record);
    }
    let accounted = summary
        .prompt_cache_hit_tokens
        .saturating_add(summary.prompt_cache_miss_tokens);
    summary.prompt_cache_hit_basis_points = if accounted == 0 {
        0
    } else {
        summary.prompt_cache_hit_tokens.saturating_mul(10_000) / accounted
    };

    for thread in threads {
        summary.turn_count = summary
            .turn_count
            .saturating_add(store.list_turns(&thread.id)?.len());
        let events = store.read_events(&thread.id, 0)?;
        accumulate_prompt_layer_events(&mut summary, &events);
        for item in store.list_items(&thread.id, None)? {
            if item.item_type == "event" {
                if item.content.contains("tool_call_repair") {
                    summary.repair_count = summary.repair_count.saturating_add(1);
                }
                accumulate_model_route_item(&mut summary, &item.content);
            }
            if item.content.contains("repeated identical")
                && (item.item_type == "tool_result" || item.item_type == "event")
            {
                summary.repeated_tool_suppressions =
                    summary.repeated_tool_suppressions.saturating_add(1);
            }
        }
        for event in events {
            if event.kind == "tool_call_repair" {
                summary.repair_count = summary.repair_count.saturating_add(1);
            }
            accumulate_model_route_event(&mut summary, &event);
            if event.kind == "tool_result"
                && json_value_to_string(&event.payload).contains("repeated identical")
            {
                summary.repeated_tool_suppressions =
                    summary.repeated_tool_suppressions.saturating_add(1);
            }
        }
    }

    Ok(summary)
}

fn selected_threads(store: &RuntimeStore, args: &StatsArgs) -> AppResult<Vec<ThreadRecord>> {
    if let Some(thread_id) = args.thread.as_deref() {
        return Ok(vec![store.load_thread(thread_id)?]);
    }
    if let Some(session_id) = args.session.as_deref() {
        store.load_session(session_id)?;
        return store.list_session_threads(session_id, usize::MAX);
    }
    store.list_threads(usize::MAX)
}

fn selected_usage(
    store: &RuntimeStore,
    args: &StatsArgs,
    thread_ids: &BTreeSet<String>,
    limit: usize,
) -> AppResult<Vec<UsageRecord>> {
    if let Some(thread_id) = args.thread.as_deref() {
        return store.list_usage(Some(thread_id), limit);
    }
    let mut records = store
        .list_usage(None, usize::MAX)?
        .into_iter()
        .filter(|record| thread_ids.contains(&record.thread_id))
        .collect::<Vec<_>>();
    records.truncate(limit);
    Ok(records)
}

fn accumulate_usage(summary: &mut StatsSummary, record: &UsageRecord) {
    summary.prompt_tokens = summary.prompt_tokens.saturating_add(record.prompt_tokens);
    summary.completion_tokens = summary
        .completion_tokens
        .saturating_add(record.completion_tokens);
    summary.total_tokens = summary.total_tokens.saturating_add(record.total_tokens);
    summary.prompt_cache_hit_tokens = summary
        .prompt_cache_hit_tokens
        .saturating_add(record.prompt_cache_hit_tokens);
    summary.prompt_cache_miss_tokens = summary
        .prompt_cache_miss_tokens
        .saturating_add(record.prompt_cache_miss_tokens);
    match (
        record.estimated_input_cost_microusd,
        record.estimated_output_cost_microusd,
        record.estimated_total_cost_microusd,
    ) {
        (Some(input), Some(output), Some(total)) => {
            summary.estimated_input_cost_microusd =
                summary.estimated_input_cost_microusd.saturating_add(input);
            summary.estimated_output_cost_microusd = summary
                .estimated_output_cost_microusd
                .saturating_add(output);
            summary.estimated_total_cost_microusd =
                summary.estimated_total_cost_microusd.saturating_add(total);
        }
        _ => summary.unpriced_record_count = summary.unpriced_record_count.saturating_add(1),
    }
    *summary
        .model_counts
        .entry(record.model.clone())
        .or_insert(0) += 1;
}

fn accumulate_model_route_event(summary: &mut StatsSummary, event: &RuntimeEvent) {
    if event.kind != "model_route" {
        return;
    }
    let Some(root) = json_as_object(&event.payload) else {
        return;
    };
    let Some(preset) = root.get("preset").and_then(json_as_string) else {
        return;
    };
    let Some(model) = root.get("model").and_then(json_as_string) else {
        return;
    };
    let escalated = root.get("escalated").and_then(json_bool).unwrap_or(false);
    accumulate_model_route(summary, preset, model, escalated);
}

fn accumulate_model_route_item(summary: &mut StatsSummary, content: &str) {
    if let Some((preset, model, escalated)) = parse_model_route_item(content) {
        accumulate_model_route(summary, &preset, &model, escalated);
    }
}

fn accumulate_model_route(summary: &mut StatsSummary, preset: &str, model: &str, escalated: bool) {
    let preset = preset.trim();
    let model = model.trim();
    if preset.is_empty() || model.is_empty() {
        return;
    }
    *summary
        .model_preset_counts
        .entry(preset.to_string())
        .or_insert(0) += 1;
    *summary
        .model_route_counts
        .entry(ModelRouteKey {
            preset: preset.to_string(),
            model: model.to_string(),
            escalated,
        })
        .or_insert(0) += 1;
}

fn parse_model_route_item(content: &str) -> Option<(String, String, bool)> {
    let mut preset = None;
    let mut model = None;
    let mut escalated = false;
    for line in content.lines().map(str::trim) {
        if let Some(value) = line.strip_prefix("model preset: ") {
            let value = value.trim();
            if !value.is_empty() {
                preset = Some(value.to_string());
            }
            continue;
        }
        if let Some(value) = line.strip_prefix("model: ") {
            let value = value.trim();
            if !value.is_empty() {
                model = Some(value.to_string());
            }
            continue;
        }
        if let Some(value) = line.strip_prefix("escalating next call to ") {
            let model_value = value
                .split_once(':')
                .map(|(head, _)| head)
                .unwrap_or(value)
                .trim();
            if !model_value.is_empty() {
                model = Some(model_value.to_string());
                escalated = true;
            }
        }
    }
    Some((preset?, model?, escalated))
}

fn accumulate_prompt_layer_events(summary: &mut StatsSummary, events: &[RuntimeEvent]) {
    for event in events {
        if event.kind != "prompt_layers_recorded" {
            continue;
        }
        let Some(root) = json_as_object(&event.payload) else {
            continue;
        };
        if let Some(digest) = root.get("digest").and_then(json_as_string) {
            summary.latest_prompt_layer_digest = Some(digest.to_string());
        }
        let Some(snapshots) = root.get("snapshots").and_then(json_as_array) else {
            continue;
        };
        summary.prompt_layer_snapshot_count = summary
            .prompt_layer_snapshot_count
            .saturating_add(snapshots.len() as u64);
        if let Some(latest) = snapshots.last().and_then(json_as_object) {
            if let Some(tokens) = latest.get("estimated_tokens").and_then(json_u64) {
                summary.latest_prompt_layer_estimated_tokens = tokens;
            }
        }
        for snapshot in snapshots {
            accumulate_prompt_layer_trends(summary, snapshot);
        }
    }
}

fn accumulate_prompt_layer_trends(summary: &mut StatsSummary, snapshot: &JsonValue) {
    let Some(root) = json_as_object(snapshot) else {
        return;
    };
    let Some(layers) = root.get("layers").and_then(json_as_array) else {
        return;
    };
    for layer in layers {
        let Some(layer) = json_as_object(layer) else {
            continue;
        };
        let Some(name) = layer.get("name").and_then(json_as_string) else {
            continue;
        };
        let tokens = layer
            .get("estimated_tokens")
            .and_then(json_u64)
            .unwrap_or_default();
        let cache_stable = layer
            .get("cache_stable")
            .and_then(json_bool)
            .unwrap_or(false);
        let hash = layer
            .get("text_sha256")
            .and_then(json_as_string)
            .map(str::to_string);
        let trend = summary
            .prompt_layer_trends
            .entry(name.to_string())
            .or_default();
        if trend.snapshot_count == 0 {
            trend.first_estimated_tokens = tokens;
        }
        trend.snapshot_count = trend.snapshot_count.saturating_add(1);
        trend.latest_estimated_tokens = tokens;
        trend.max_estimated_tokens = trend.max_estimated_tokens.max(tokens);
        trend.cache_stable = cache_stable;

        if let (Some(previous), Some(current)) = (trend.previous_hash.as_deref(), hash.as_deref()) {
            if previous != current {
                trend.hash_changes = trend.hash_changes.saturating_add(1);
                if cache_stable {
                    summary.prompt_layer_cache_stable_hash_changes = summary
                        .prompt_layer_cache_stable_hash_changes
                        .saturating_add(1);
                }
            }
        }
        if hash.is_some() {
            trend.previous_hash = hash.clone();
            trend.latest_hash = hash;
        }
    }
}

fn stats_scope(args: &StatsArgs) -> String {
    if let Some(thread) = args.thread.as_deref() {
        format!("thread {thread}")
    } else if let Some(session) = args.session.as_deref() {
        format!("session {session}")
    } else {
        "all runtime threads".to_string()
    }
}

fn render_stats_summary(summary: &StatsSummary) -> String {
    let mut out = String::new();
    out.push_str("DeepSeekCode stats\n");
    out.push_str(&format!("scope: {}\n", summary.scope));
    out.push_str(&format!("threads: {}\n", summary.thread_count));
    out.push_str(&format!("turns: {}\n", summary.turn_count));
    out.push_str(&format!("model_turns: {}\n", summary.model_turns));
    out.push_str(&format!("prompt_tokens: {}\n", summary.prompt_tokens));
    out.push_str(&format!(
        "completion_tokens: {}\n",
        summary.completion_tokens
    ));
    out.push_str(&format!("total_tokens: {}\n", summary.total_tokens));
    out.push_str(&format!(
        "prompt_cache_hit_tokens: {}\n",
        summary.prompt_cache_hit_tokens
    ));
    out.push_str(&format!(
        "prompt_cache_miss_tokens: {}\n",
        summary.prompt_cache_miss_tokens
    ));
    out.push_str(&format!(
        "prompt_cache_hit_rate: {}\n",
        basis_points_percent(summary.prompt_cache_hit_basis_points)
    ));
    out.push_str(&format!(
        "estimated_input_cost_usd: {}\n",
        microusd_decimal(summary.estimated_input_cost_microusd)
    ));
    out.push_str(&format!(
        "estimated_output_cost_usd: {}\n",
        microusd_decimal(summary.estimated_output_cost_microusd)
    ));
    out.push_str(&format!(
        "estimated_total_cost_usd: {}\n",
        microusd_decimal(summary.estimated_total_cost_microusd)
    ));
    if summary.unpriced_record_count > 0 {
        out.push_str(&format!(
            "unpriced_records: {}\n",
            summary.unpriced_record_count
        ));
    }
    out.push_str(&format!("repair_count: {}\n", summary.repair_count));
    out.push_str(&format!(
        "repeated_tool_suppressions: {}\n",
        summary.repeated_tool_suppressions
    ));
    out.push_str(&format!(
        "prompt_layer_snapshots: {}\n",
        summary.prompt_layer_snapshot_count
    ));
    out.push_str(&format!(
        "latest_prompt_layer_estimated_tokens: {}\n",
        summary.latest_prompt_layer_estimated_tokens
    ));
    if let Some(digest) = summary.latest_prompt_layer_digest.as_deref() {
        out.push_str(&format!("latest_prompt_layer_digest: {digest}\n"));
    }
    out.push_str(&format!(
        "prompt_layer_cache_stable_hash_changes: {}\n",
        summary.prompt_layer_cache_stable_hash_changes
    ));
    if !summary.prompt_layer_trends.is_empty() {
        out.push_str("prompt_layer_trends:\n");
        for (name, trend) in &summary.prompt_layer_trends {
            out.push_str(&format!(
                "- {name}: snapshots={} tokens={}->{} delta={} max={} hash_changes={} cache_stable={}\n",
                trend.snapshot_count,
                trend.first_estimated_tokens,
                trend.latest_estimated_tokens,
                signed_token_delta(trend),
                trend.max_estimated_tokens,
                trend.hash_changes,
                trend.cache_stable
            ));
        }
    }
    if !summary.model_counts.is_empty() {
        out.push_str("models:\n");
        for (model, count) in &summary.model_counts {
            out.push_str(&format!("- {model}: {count}\n"));
        }
    }
    if !summary.model_preset_counts.is_empty() {
        out.push_str("model_presets:\n");
        for (preset, count) in &summary.model_preset_counts {
            out.push_str(&format!("- {preset}: {count}\n"));
        }
    }
    if !summary.model_route_counts.is_empty() {
        out.push_str("model_routes:\n");
        for (route, count) in &summary.model_route_counts {
            let suffix = if route.escalated { " (escalated)" } else { "" };
            out.push_str(&format!(
                "- {} -> {}{}: {}\n",
                route.preset, route.model, suffix, count
            ));
        }
    }
    out.trim_end().to_string()
}

fn stats_summary_to_json(summary: &StatsSummary) -> JsonValue {
    JsonValue::Object(
        [
            (
                "schema".to_string(),
                JsonValue::String("deepseek.stats.v1".to_string()),
            ),
            (
                "scope".to_string(),
                JsonValue::String(summary.scope.clone()),
            ),
            (
                "thread_count".to_string(),
                JsonValue::Number(summary.thread_count.to_string()),
            ),
            (
                "turn_count".to_string(),
                JsonValue::Number(summary.turn_count.to_string()),
            ),
            (
                "model_turns".to_string(),
                JsonValue::Number(summary.model_turns.to_string()),
            ),
            (
                "prompt_tokens".to_string(),
                JsonValue::Number(summary.prompt_tokens.to_string()),
            ),
            (
                "completion_tokens".to_string(),
                JsonValue::Number(summary.completion_tokens.to_string()),
            ),
            (
                "total_tokens".to_string(),
                JsonValue::Number(summary.total_tokens.to_string()),
            ),
            (
                "prompt_cache_hit_tokens".to_string(),
                JsonValue::Number(summary.prompt_cache_hit_tokens.to_string()),
            ),
            (
                "prompt_cache_miss_tokens".to_string(),
                JsonValue::Number(summary.prompt_cache_miss_tokens.to_string()),
            ),
            (
                "prompt_cache_hit_basis_points".to_string(),
                JsonValue::Number(summary.prompt_cache_hit_basis_points.to_string()),
            ),
            (
                "estimated_input_cost_microusd".to_string(),
                JsonValue::Number(summary.estimated_input_cost_microusd.to_string()),
            ),
            (
                "estimated_output_cost_microusd".to_string(),
                JsonValue::Number(summary.estimated_output_cost_microusd.to_string()),
            ),
            (
                "estimated_total_cost_microusd".to_string(),
                JsonValue::Number(summary.estimated_total_cost_microusd.to_string()),
            ),
            (
                "unpriced_record_count".to_string(),
                JsonValue::Number(summary.unpriced_record_count.to_string()),
            ),
            (
                "repair_count".to_string(),
                JsonValue::Number(summary.repair_count.to_string()),
            ),
            (
                "repeated_tool_suppressions".to_string(),
                JsonValue::Number(summary.repeated_tool_suppressions.to_string()),
            ),
            (
                "prompt_layer_snapshot_count".to_string(),
                JsonValue::Number(summary.prompt_layer_snapshot_count.to_string()),
            ),
            (
                "latest_prompt_layer_estimated_tokens".to_string(),
                JsonValue::Number(summary.latest_prompt_layer_estimated_tokens.to_string()),
            ),
            (
                "latest_prompt_layer_digest".to_string(),
                summary
                    .latest_prompt_layer_digest
                    .as_ref()
                    .map(|value| JsonValue::String(value.clone()))
                    .unwrap_or(JsonValue::Null),
            ),
            (
                "prompt_layer_cache_stable_hash_changes".to_string(),
                JsonValue::Number(summary.prompt_layer_cache_stable_hash_changes.to_string()),
            ),
            (
                "prompt_layer_trends".to_string(),
                prompt_layer_trends_to_json(&summary.prompt_layer_trends),
            ),
            (
                "models".to_string(),
                JsonValue::Object(
                    summary
                        .model_counts
                        .iter()
                        .map(|(model, count)| (model.clone(), JsonValue::Number(count.to_string())))
                        .collect(),
                ),
            ),
            (
                "model_presets".to_string(),
                JsonValue::Object(
                    summary
                        .model_preset_counts
                        .iter()
                        .map(|(preset, count)| {
                            (preset.clone(), JsonValue::Number(count.to_string()))
                        })
                        .collect(),
                ),
            ),
            (
                "model_routes".to_string(),
                model_routes_to_json(&summary.model_route_counts),
            ),
        ]
        .into_iter()
        .collect(),
    )
}

fn model_routes_to_json(routes: &BTreeMap<ModelRouteKey, u64>) -> JsonValue {
    JsonValue::Array(
        routes
            .iter()
            .map(|(route, count)| {
                JsonValue::Object(
                    [
                        (
                            "preset".to_string(),
                            JsonValue::String(route.preset.clone()),
                        ),
                        ("model".to_string(), JsonValue::String(route.model.clone())),
                        ("escalated".to_string(), JsonValue::Bool(route.escalated)),
                        ("count".to_string(), JsonValue::Number(count.to_string())),
                    ]
                    .into_iter()
                    .collect(),
                )
            })
            .collect(),
    )
}

fn prompt_layer_trends_to_json(trends: &BTreeMap<String, PromptLayerTrend>) -> JsonValue {
    JsonValue::Array(
        trends
            .iter()
            .map(|(name, trend)| {
                JsonValue::Object(
                    [
                        ("name".to_string(), JsonValue::String(name.clone())),
                        (
                            "cache_stable".to_string(),
                            JsonValue::Bool(trend.cache_stable),
                        ),
                        (
                            "snapshot_count".to_string(),
                            JsonValue::Number(trend.snapshot_count.to_string()),
                        ),
                        (
                            "first_estimated_tokens".to_string(),
                            JsonValue::Number(trend.first_estimated_tokens.to_string()),
                        ),
                        (
                            "latest_estimated_tokens".to_string(),
                            JsonValue::Number(trend.latest_estimated_tokens.to_string()),
                        ),
                        (
                            "max_estimated_tokens".to_string(),
                            JsonValue::Number(trend.max_estimated_tokens.to_string()),
                        ),
                        (
                            "token_delta".to_string(),
                            JsonValue::Number(signed_token_delta(trend).to_string()),
                        ),
                        (
                            "hash_changes".to_string(),
                            JsonValue::Number(trend.hash_changes.to_string()),
                        ),
                        (
                            "latest_hash".to_string(),
                            trend
                                .latest_hash
                                .as_ref()
                                .map(|value| JsonValue::String(value.clone()))
                                .unwrap_or(JsonValue::Null),
                        ),
                    ]
                    .into_iter()
                    .collect(),
                )
            })
            .collect(),
    )
}

fn signed_token_delta(trend: &PromptLayerTrend) -> i128 {
    i128::from(trend.latest_estimated_tokens) - i128::from(trend.first_estimated_tokens)
}

fn require_prompt_prefix_stable(summary: &StatsSummary) -> AppResult<()> {
    if summary.prompt_layer_snapshot_count == 0 {
        return Err(app_error(
            "prompt prefix stability gate failed: no prompt-layer snapshots recorded",
        ));
    }
    if summary.prompt_layer_cache_stable_hash_changes == 0 {
        return Ok(());
    }
    let changed_layers = summary
        .prompt_layer_trends
        .iter()
        .filter_map(|(name, trend)| {
            (trend.cache_stable && trend.hash_changes > 0)
                .then(|| format!("{name}:{}", trend.hash_changes))
        })
        .collect::<Vec<_>>();
    let suffix = if changed_layers.is_empty() {
        "inspect prompt_layer_trends for changed cache-stable layers".to_string()
    } else {
        format!("changed cache-stable layers: {}", changed_layers.join(", "))
    };
    Err(app_error(format!(
        "prompt prefix stability gate failed: {} cache-stable layer hash change(s); {suffix}",
        summary.prompt_layer_cache_stable_hash_changes
    )))
}

fn basis_points_percent(value: u64) -> String {
    format!("{}.{:02}%", value / 100, value % 100)
}

fn microusd_decimal(value: u64) -> String {
    format!("{}.{:06}", value / 1_000_000, value % 1_000_000)
}

fn json_u64(value: &JsonValue) -> Option<u64> {
    match value {
        JsonValue::Number(raw) => raw.parse::<u64>().ok(),
        _ => None,
    }
}

fn json_bool(value: &JsonValue) -> Option<bool> {
    match value {
        JsonValue::Bool(value) => Some(*value),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::runtime::{json_array, json_object, RuntimeStore};

    fn temp_store(label: &str) -> RuntimeStore {
        RuntimeStore::new(std::env::temp_dir().join(format!(
            "deepseek-stats-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        )))
    }

    #[test]
    fn stats_summary_aggregates_usage_cache_events_and_prompt_layers() {
        let store = temp_store("aggregate");
        let session = store
            .create_session("Stats".to_string(), ".".to_string())
            .unwrap();
        let thread = store
            .create_thread_for_session(
                &session.id,
                "Stats".to_string(),
                ".".to_string(),
                "deepseek-v4-pro".to_string(),
                "agent".to_string(),
            )
            .unwrap();
        let turn = store
            .append_turn(&thread.id, "assistant".to_string(), "done".to_string())
            .unwrap();
        let usage = store
            .append_usage_with_cache(
                &thread.id,
                Some(&turn.id),
                "deepseek-v4-pro".to_string(),
                "test".to_string(),
                100,
                25,
                70,
                30,
            )
            .unwrap();
        store
            .append_thread_event(
                &thread.id,
                "prompt_layers_recorded",
                json_object([
                    (
                        "type",
                        JsonValue::String("prompt_layers_recorded".to_string()),
                    ),
                    ("usage_id", JsonValue::String(usage.id)),
                    ("digest", JsonValue::String("abc123".to_string())),
                    (
                        "snapshots",
                        json_array(vec![
                            json_object([
                                ("estimated_tokens", JsonValue::Number("80".to_string())),
                                (
                                    "layers",
                                    json_array(vec![
                                        json_object([
                                            (
                                                "name",
                                                JsonValue::String("system_static".to_string()),
                                            ),
                                            (
                                                "text_sha256",
                                                JsonValue::String("stable-a".to_string()),
                                            ),
                                            (
                                                "estimated_tokens",
                                                JsonValue::Number("50".to_string()),
                                            ),
                                            ("cache_stable", JsonValue::Bool(true)),
                                        ]),
                                        json_object([
                                            (
                                                "name",
                                                JsonValue::String("append_only_turns".to_string()),
                                            ),
                                            (
                                                "text_sha256",
                                                JsonValue::String("turns-a".to_string()),
                                            ),
                                            (
                                                "estimated_tokens",
                                                JsonValue::Number("30".to_string()),
                                            ),
                                            ("cache_stable", JsonValue::Bool(false)),
                                        ]),
                                    ]),
                                ),
                            ]),
                            json_object([
                                ("estimated_tokens", JsonValue::Number("88".to_string())),
                                (
                                    "layers",
                                    json_array(vec![
                                        json_object([
                                            (
                                                "name",
                                                JsonValue::String("system_static".to_string()),
                                            ),
                                            (
                                                "text_sha256",
                                                JsonValue::String("stable-b".to_string()),
                                            ),
                                            (
                                                "estimated_tokens",
                                                JsonValue::Number("52".to_string()),
                                            ),
                                            ("cache_stable", JsonValue::Bool(true)),
                                        ]),
                                        json_object([
                                            (
                                                "name",
                                                JsonValue::String("append_only_turns".to_string()),
                                            ),
                                            (
                                                "text_sha256",
                                                JsonValue::String("turns-b".to_string()),
                                            ),
                                            (
                                                "estimated_tokens",
                                                JsonValue::Number("36".to_string()),
                                            ),
                                            ("cache_stable", JsonValue::Bool(false)),
                                        ]),
                                    ]),
                                ),
                            ]),
                        ]),
                    ),
                ]),
            )
            .unwrap();
        store
            .append_item(
                &thread.id,
                Some(&turn.id),
                "event".to_string(),
                Some("system".to_string()),
                "tool_call_repair kind=truncated-json".to_string(),
                "completed".to_string(),
            )
            .unwrap();
        store
            .append_item(
                &thread.id,
                Some(&turn.id),
                "event".to_string(),
                Some("system".to_string()),
                "model preset: flash\nmodel: deepseek-v4-flash\nreason: explicit preset"
                    .to_string(),
                "completed".to_string(),
            )
            .unwrap();
        store
            .append_thread_event(
                &thread.id,
                "model_route",
                json_object([
                    ("type", JsonValue::String("model_route".to_string())),
                    ("preset", JsonValue::String("auto".to_string())),
                    ("model", JsonValue::String("deepseek-v4-pro".to_string())),
                    (
                        "reason",
                        JsonValue::String("repeated repair signals".to_string()),
                    ),
                    ("escalated", JsonValue::Bool(true)),
                ]),
            )
            .unwrap();
        store
            .append_item(
                &thread.id,
                Some(&turn.id),
                "tool_result".to_string(),
                Some("tool".to_string()),
                "repeated identical mutating or side-effecting tool call suppressed".to_string(),
                "failed".to_string(),
            )
            .unwrap();

        let summary = stats_summary(
            &store,
            &StatsArgs {
                session: Some(session.id),
                ..StatsArgs::default()
            },
        )
        .unwrap();
        assert_eq!(summary.thread_count, 1);
        assert_eq!(summary.turn_count, 1);
        assert_eq!(summary.model_turns, 1);
        assert_eq!(summary.prompt_tokens, 100);
        assert_eq!(summary.prompt_cache_hit_basis_points, 7000);
        assert_eq!(summary.repair_count, 1);
        assert_eq!(summary.repeated_tool_suppressions, 1);
        assert_eq!(summary.prompt_layer_snapshot_count, 2);
        assert_eq!(summary.latest_prompt_layer_estimated_tokens, 88);
        assert_eq!(
            summary.latest_prompt_layer_digest.as_deref(),
            Some("abc123")
        );
        assert_eq!(summary.prompt_layer_cache_stable_hash_changes, 1);
        assert_eq!(summary.model_preset_counts.get("auto"), Some(&1));
        assert_eq!(summary.model_preset_counts.get("flash"), Some(&1));
        assert_eq!(
            summary.model_route_counts.get(&ModelRouteKey {
                preset: "auto".to_string(),
                model: "deepseek-v4-pro".to_string(),
                escalated: true,
            }),
            Some(&1)
        );
        assert_eq!(
            summary.model_route_counts.get(&ModelRouteKey {
                preset: "flash".to_string(),
                model: "deepseek-v4-flash".to_string(),
                escalated: false,
            }),
            Some(&1)
        );
        let system = summary
            .prompt_layer_trends
            .get("system_static")
            .expect("expected system_static trend");
        assert_eq!(system.snapshot_count, 2);
        assert_eq!(system.hash_changes, 1);
        assert_eq!(signed_token_delta(system), 2);
        let turns = summary
            .prompt_layer_trends
            .get("append_only_turns")
            .expect("expected append_only_turns trend");
        assert_eq!(turns.hash_changes, 1);
        assert!(!turns.cache_stable);
    }

    #[test]
    fn render_stats_summary_includes_cache_and_model_split() {
        let mut summary = StatsSummary {
            scope: "thread thread-1".to_string(),
            thread_count: 1,
            turn_count: 3,
            model_turns: 2,
            prompt_cache_hit_basis_points: 7550,
            estimated_input_cost_microusd: 234,
            estimated_output_cost_microusd: 1000,
            estimated_total_cost_microusd: 1234,
            ..StatsSummary::default()
        };
        summary
            .model_counts
            .insert("deepseek-v4-flash".to_string(), 2);
        summary.model_preset_counts.insert("flash".to_string(), 2);
        summary.model_route_counts.insert(
            ModelRouteKey {
                preset: "flash".to_string(),
                model: "deepseek-v4-flash".to_string(),
                escalated: false,
            },
            2,
        );
        summary.model_route_counts.insert(
            ModelRouteKey {
                preset: "auto".to_string(),
                model: "deepseek-v4-pro".to_string(),
                escalated: true,
            },
            1,
        );
        let rendered = render_stats_summary(&summary);
        assert!(rendered.contains("prompt_cache_hit_rate: 75.50%"));
        assert!(rendered.contains("turns: 3"));
        assert!(rendered.contains("estimated_input_cost_usd: 0.000234"));
        assert!(rendered.contains("estimated_output_cost_usd: 0.001000"));
        assert!(rendered.contains("estimated_total_cost_usd: 0.001234"));
        assert!(rendered.contains("prompt_layer_cache_stable_hash_changes: 0"));
        assert!(rendered.contains("- deepseek-v4-flash: 2"));
        assert!(rendered.contains("model_presets:"));
        assert!(rendered.contains("- flash: 2"));
        assert!(rendered.contains("model_routes:"));
        assert!(rendered.contains("- auto -> deepseek-v4-pro (escalated): 1"));
        assert!(rendered.contains("- flash -> deepseek-v4-flash: 2"));
    }

    #[test]
    fn prefix_stability_gate_fails_without_prompt_layer_evidence() {
        let error = require_prompt_prefix_stable(&StatsSummary::default()).unwrap_err();
        assert!(error
            .to_string()
            .contains("no prompt-layer snapshots recorded"));
    }

    #[test]
    fn prefix_stability_gate_fails_on_cache_stable_hash_change() {
        let mut summary = StatsSummary {
            prompt_layer_snapshot_count: 2,
            prompt_layer_cache_stable_hash_changes: 1,
            ..StatsSummary::default()
        };
        summary.prompt_layer_trends.insert(
            "system_static".to_string(),
            PromptLayerTrend {
                cache_stable: true,
                hash_changes: 1,
                ..PromptLayerTrend::default()
            },
        );

        let error = require_prompt_prefix_stable(&summary).unwrap_err();
        assert!(error
            .to_string()
            .contains("prompt prefix stability gate failed"));
        assert!(error.to_string().contains("system_static:1"));
    }

    #[test]
    fn stats_rejects_missing_thread() {
        let store = temp_store("missing-thread");
        let error = stats_summary(
            &store,
            &StatsArgs {
                thread: Some("thread-missing".to_string()),
                ..StatsArgs::default()
            },
        )
        .unwrap_err();
        assert!(error.to_string().contains("runtime thread not found"));
    }
}
