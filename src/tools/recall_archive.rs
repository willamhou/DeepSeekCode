use std::path::PathBuf;

use crate::config::types::AppConfig;
use crate::core::runtime::{json_array, json_object, RuntimeStore, ThreadRecord, TurnRecord};
use crate::error::{app_error, AppResult};
use crate::tools::types::{Tool, ToolInput, ToolOutput};
use crate::util::json::{json_value_to_string, JsonValue};

const DEFAULT_MAX_RESULTS: usize = 3;
const HARD_MAX_RESULTS: usize = 10;
const THREAD_SCAN_LIMIT: usize = 100;
const EXCERPT_CONTEXT_CHARS: usize = 160;

pub struct RecallArchiveTool {
    store: RuntimeStore,
}

impl RecallArchiveTool {
    pub fn new(config: &AppConfig) -> Self {
        Self {
            store: RuntimeStore::new(PathBuf::from(&config.workspace.config_dir).join("runtime")),
        }
    }
}

impl Tool for RecallArchiveTool {
    fn name(&self) -> &str {
        "recall_archive"
    }

    fn execute(&self, input: ToolInput) -> AppResult<ToolOutput> {
        let query = required_nonempty(&input, "query")?;
        let terms = tokenize(&query);
        if terms.is_empty() {
            return Err(app_error(
                "recall_archive query has no searchable tokens after tokenization",
            ));
        }
        let max_results = parse_limit(&input, DEFAULT_MAX_RESULTS, HARD_MAX_RESULTS);
        let threads = if let Some(thread_id) = optional_arg(&input, "thread_id") {
            vec![self.store.load_thread(thread_id)?]
        } else {
            self.store.list_threads(THREAD_SCAN_LIMIT)?
        };

        let mut hits = Vec::new();
        let mut messages_scanned = 0usize;
        for thread in &threads {
            for turn in self.store.list_turns(&thread.id)? {
                messages_scanned += 1;
                if let Some(hit) = score_turn(thread, &turn, &terms) {
                    hits.push(hit);
                }
            }
            for item in self.store.list_items(&thread.id, None)? {
                messages_scanned += 1;
                if let Some(hit) = score_item(thread, &item, &terms) {
                    hits.push(hit);
                }
            }
        }

        hits.sort_by(|left, right| {
            hit_score(right)
                .partial_cmp(&hit_score(left))
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| hit_thread_id(left).cmp(&hit_thread_id(right)))
        });
        hits.truncate(max_results);

        Ok(json_output(json_object([
            ("query", JsonValue::String(query)),
            (
                "threads_searched",
                JsonValue::Number(threads.len().to_string()),
            ),
            (
                "messages_scanned",
                JsonValue::Number(messages_scanned.to_string()),
            ),
            ("hits", json_array(hits)),
        ])))
    }
}

fn score_turn(thread: &ThreadRecord, turn: &TurnRecord, terms: &[String]) -> Option<JsonValue> {
    let score = score_text(&turn.content, terms)?;
    Some(json_object([
        ("thread_id", JsonValue::String(thread.id.clone())),
        ("thread_title", JsonValue::String(thread.title.clone())),
        ("source", JsonValue::String("turn".to_string())),
        ("turn_id", JsonValue::String(turn.id.clone())),
        ("item_id", JsonValue::Null),
        ("role", JsonValue::String(turn.role.clone())),
        ("score", JsonValue::Number(format_score(score))),
        ("excerpt", JsonValue::String(excerpt(&turn.content, terms))),
    ]))
}

fn score_item(
    thread: &ThreadRecord,
    item: &crate::core::runtime::ItemRecord,
    terms: &[String],
) -> Option<JsonValue> {
    let score = score_text(&item.content, terms)?;
    Some(json_object([
        ("thread_id", JsonValue::String(thread.id.clone())),
        ("thread_title", JsonValue::String(thread.title.clone())),
        ("source", JsonValue::String(item.item_type.clone())),
        (
            "turn_id",
            item.turn_id
                .clone()
                .map(JsonValue::String)
                .unwrap_or(JsonValue::Null),
        ),
        ("item_id", JsonValue::String(item.id.clone())),
        (
            "role",
            item.role
                .clone()
                .map(JsonValue::String)
                .unwrap_or(JsonValue::Null),
        ),
        ("score", JsonValue::Number(format_score(score))),
        ("excerpt", JsonValue::String(excerpt(&item.content, terms))),
    ]))
}

fn score_text(text: &str, terms: &[String]) -> Option<f64> {
    let lower = text.to_ascii_lowercase();
    let mut score = 0.0;
    for term in terms {
        let count = lower.matches(term).count();
        if count > 0 {
            score += 1.0 + count as f64;
        }
    }
    if score == 0.0 {
        None
    } else {
        Some(score / lower.len().max(1) as f64 * 1000.0)
    }
}

fn tokenize(query: &str) -> Vec<String> {
    query
        .split(|ch: char| !ch.is_ascii_alphanumeric() && ch != '_' && ch != '-')
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| value.len() >= 2)
        .collect()
}

fn excerpt(text: &str, terms: &[String]) -> String {
    let lower = text.to_ascii_lowercase();
    let first_match = terms
        .iter()
        .filter_map(|term| lower.find(term))
        .min()
        .unwrap_or(0);
    let start = text[..first_match]
        .char_indices()
        .rev()
        .nth(EXCERPT_CONTEXT_CHARS)
        .map(|(idx, _)| idx)
        .unwrap_or(0);
    let end = text[first_match..]
        .char_indices()
        .nth(EXCERPT_CONTEXT_CHARS)
        .map(|(idx, _)| first_match + idx)
        .unwrap_or(text.len());
    let mut out = String::new();
    if start > 0 {
        out.push_str("...");
    }
    out.push_str(text[start..end].trim());
    if end < text.len() {
        out.push_str("...");
    }
    out
}

fn hit_score(value: &JsonValue) -> f64 {
    let JsonValue::Object(root) = value else {
        return 0.0;
    };
    root.get("score")
        .and_then(|value| match value {
            JsonValue::Number(raw) => raw.parse::<f64>().ok(),
            _ => None,
        })
        .unwrap_or(0.0)
}

fn hit_thread_id(value: &JsonValue) -> String {
    let JsonValue::Object(root) = value else {
        return String::new();
    };
    root.get("thread_id")
        .and_then(|value| match value {
            JsonValue::String(raw) => Some(raw.clone()),
            _ => None,
        })
        .unwrap_or_default()
}

fn format_score(score: f64) -> String {
    format!("{score:.3}")
}

fn required_nonempty(input: &ToolInput, key: &str) -> AppResult<String> {
    input
        .get(key)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .ok_or_else(|| app_error(format!("recall_archive requires `{key}`")))
}

fn optional_arg<'a>(input: &'a ToolInput, key: &str) -> Option<&'a str> {
    input
        .get(key)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn parse_limit(input: &ToolInput, default: usize, max: usize) -> usize {
    input
        .get("max_results")
        .or_else(|| input.get("limit"))
        .and_then(|value| value.trim().parse::<usize>().ok())
        .unwrap_or(default)
        .clamp(1, max)
}

fn json_output(value: JsonValue) -> ToolOutput {
    ToolOutput {
        summary: json_value_to_string(&value),
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
            "deepseek-recall-archive-{name}-{}-{suffix}",
            std::process::id()
        ))
    }

    fn config_for(root: &std::path::Path) -> AppConfig {
        let mut config = AppConfig::default();
        config.workspace.config_dir = root.join(".dscode").display().to_string();
        config
    }

    #[test]
    fn recall_archive_searches_runtime_turns_and_items() {
        let root = temp_root("runtime-search");
        let config = config_for(&root);
        let store = RuntimeStore::new(root.join(".dscode/runtime"));
        let thread = store
            .create_thread(
                "Investigate billing".to_string(),
                ".".to_string(),
                "deepseek-coder".to_string(),
                "agent".to_string(),
            )
            .unwrap();
        let turn = store
            .append_turn(
                &thread.id,
                "assistant".to_string(),
                "The Stripe reconciliation bug is in invoice sync.".to_string(),
            )
            .unwrap();
        store
            .append_item(
                &thread.id,
                Some(&turn.id),
                "summary".to_string(),
                Some("assistant".to_string()),
                "Carry forward: investigate invoice sync reconciliation.".to_string(),
                "completed".to_string(),
            )
            .unwrap();

        let output = RecallArchiveTool::new(&config)
            .execute(
                ToolInput::new()
                    .with_arg("query", "invoice reconciliation")
                    .with_arg("max_results", "2"),
            )
            .unwrap();

        assert!(output.summary.contains("\"threads_searched\":1"));
        assert!(output.summary.contains("\"messages_scanned\":2"));
        assert!(output.summary.contains("\"hits\":["));
        assert!(output.summary.contains("invoice sync"));

        let _ = std::fs::remove_dir_all(root);
    }
}
