use crate::core::runtime::{json_array, json_object};
use crate::error::{app_error, AppResult};
use crate::model::deepseek::static_tool_search_catalog;
use crate::tools::types::{Tool, ToolInput, ToolOutput};
use crate::util::json::{json_value_to_string, JsonValue};

const DEFAULT_LIMIT: usize = 5;
const MAX_LIMIT: usize = 20;

#[derive(Debug, Clone, Copy)]
pub enum ToolSearchMode {
    Regex,
    Bm25,
}

pub struct ToolSearchTool {
    pub tool_name: &'static str,
    pub mode: ToolSearchMode,
}

impl Tool for ToolSearchTool {
    fn name(&self) -> &str {
        self.tool_name
    }

    fn execute(&self, input: ToolInput) -> AppResult<ToolOutput> {
        let query = input
            .get("query")
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| app_error(format!("{} requires `query`", self.tool_name)))?;
        let limit = parse_limit(&input);
        let matches = match self.mode {
            ToolSearchMode::Regex => discover_tools_with_pattern(query, limit),
            ToolSearchMode::Bm25 => discover_tools_with_bm25(query, limit),
        };
        Ok(ToolOutput {
            summary: json_value_to_string(&tool_search_result(self.tool_name, query, &matches)),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ToolSearchHit {
    name: String,
    description: String,
}

fn parse_limit(input: &ToolInput) -> usize {
    input
        .get("limit")
        .and_then(|value| value.trim().parse::<usize>().ok())
        .unwrap_or(DEFAULT_LIMIT)
        .clamp(1, MAX_LIMIT)
}

fn discover_tools_with_pattern(query: &str, limit: usize) -> Vec<ToolSearchHit> {
    let query = query.to_ascii_lowercase();
    let mut hits = Vec::new();
    for (name, description, schema) in static_tool_search_catalog() {
        if is_tool_search_tool(name) {
            continue;
        }
        let haystack = tool_haystack(name, description, schema);
        if pattern_matches(&haystack, &query) {
            hits.push(ToolSearchHit {
                name: name.to_string(),
                description: description.to_string(),
            });
        }
        if hits.len() >= limit {
            break;
        }
    }
    hits
}

fn discover_tools_with_bm25(query: &str, limit: usize) -> Vec<ToolSearchHit> {
    let terms = tokenize(query);
    if terms.is_empty() {
        return Vec::new();
    }
    let phrase = query.to_ascii_lowercase();
    let mut scored = Vec::new();
    for (name, description, schema) in static_tool_search_catalog() {
        if is_tool_search_tool(name) {
            continue;
        }
        let name_lower = name.to_ascii_lowercase();
        let description_lower = description.to_ascii_lowercase();
        let schema_lower = schema.to_ascii_lowercase();
        let mut score = 0usize;
        if name_lower.contains(&phrase) {
            score += 8;
        }
        if description_lower.contains(&phrase) {
            score += 4;
        }
        for term in &terms {
            if name_lower.contains(term) {
                score += 5;
            }
            if description_lower.contains(term) {
                score += 2;
            }
            if schema_lower.contains(term) {
                score += 1;
            }
        }
        if score > 0 {
            scored.push((
                score,
                ToolSearchHit {
                    name: name.to_string(),
                    description: description.to_string(),
                },
            ));
        }
    }
    scored.sort_by(|left, right| {
        right
            .0
            .cmp(&left.0)
            .then_with(|| left.1.name.cmp(&right.1.name))
    });
    scored.into_iter().take(limit).map(|(_, hit)| hit).collect()
}

fn tool_search_result(tool_name: &str, query: &str, hits: &[ToolSearchHit]) -> JsonValue {
    json_object([
        (
            "type",
            JsonValue::String("tool_search_tool_search_result".to_string()),
        ),
        ("tool", JsonValue::String(tool_name.to_string())),
        ("query", JsonValue::String(query.to_string())),
        ("count", JsonValue::Number(hits.len().to_string())),
        (
            "tool_references",
            json_array(
                hits.iter()
                    .map(|hit| {
                        json_object([
                            ("type", JsonValue::String("tool_reference".to_string())),
                            ("tool_name", JsonValue::String(hit.name.clone())),
                            ("description", JsonValue::String(hit.description.clone())),
                        ])
                    })
                    .collect(),
            ),
        ),
    ])
}

fn tool_haystack(name: &str, description: &str, schema: &str) -> String {
    format!(
        "{}\n{}\n{}",
        name.to_ascii_lowercase(),
        description.to_ascii_lowercase(),
        schema.to_ascii_lowercase()
    )
}

fn pattern_matches(haystack: &str, query: &str) -> bool {
    query
        .split('|')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .any(|part| pattern_alternative_matches(haystack, part))
}

fn pattern_alternative_matches(haystack: &str, pattern: &str) -> bool {
    let starts = pattern.starts_with('^');
    let ends = pattern.ends_with('$');
    let body = pattern.trim_start_matches('^').trim_end_matches('$');
    if body.is_empty() {
        return false;
    }
    if starts && ends && !body.contains(".*") && !body.contains('.') {
        return haystack == body;
    }
    if starts && !body.contains(".*") && !body.contains('.') {
        return haystack.starts_with(body);
    }
    if ends && !body.contains(".*") && !body.contains('.') {
        return haystack.ends_with(body);
    }
    let dot_wildcard = body.contains('.') && !body.contains(".*");
    if dot_wildcard {
        return contains_with_single_char_wildcards(haystack, body);
    }
    if !body.contains(".*") {
        return haystack.contains(body);
    }

    let mut cursor = 0usize;
    for part in body.split(".*").filter(|part| !part.is_empty()) {
        let Some(offset) = haystack[cursor..].find(part) else {
            return false;
        };
        cursor += offset + part.len();
    }
    if starts {
        let first = body.split(".*").find(|part| !part.is_empty()).unwrap_or("");
        if !haystack.starts_with(first) {
            return false;
        }
    }
    if ends {
        let last = body
            .rsplit(".*")
            .find(|part| !part.is_empty())
            .unwrap_or("");
        if !haystack.ends_with(last) {
            return false;
        }
    }
    true
}

fn contains_with_single_char_wildcards(haystack: &str, pattern: &str) -> bool {
    let hay = haystack.as_bytes();
    let pat = pattern.as_bytes();
    if pat.len() > hay.len() {
        return false;
    }
    hay.windows(pat.len()).any(|window| {
        window
            .iter()
            .zip(pat.iter())
            .all(|(actual, expected)| *expected == b'.' || actual == expected)
    })
}

fn tokenize(query: &str) -> Vec<String> {
    query
        .split(|ch: char| !ch.is_ascii_alphanumeric() && ch != '_')
        .map(str::trim)
        .filter(|term| !term.is_empty())
        .map(str::to_ascii_lowercase)
        .collect()
}

fn is_tool_search_tool(name: &str) -> bool {
    matches!(name, "tool_search_tool_regex" | "tool_search_tool_bm25")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn regex_tool_search_returns_matching_tool_references() {
        let output = ToolSearchTool {
            tool_name: "tool_search_tool_regex",
            mode: ToolSearchMode::Regex,
        }
        .execute(ToolInput::new().with_arg("query", "github_.*context"))
        .unwrap();

        assert!(output
            .summary
            .contains("\"type\":\"tool_search_tool_search_result\""));
        assert!(output
            .summary
            .contains("\"tool_name\":\"github_issue_context\""));
        assert!(output
            .summary
            .contains("\"tool_name\":\"github_pr_context\""));
    }

    #[test]
    fn bm25_tool_search_ranks_semantic_matches() {
        let output = ToolSearchTool {
            tool_name: "tool_search_tool_bm25",
            mode: ToolSearchMode::Bm25,
        }
        .execute(ToolInput::new().with_arg("query", "market quote"))
        .unwrap();

        assert!(output.summary.contains("\"tool_name\":\"finance\""));
        assert!(!output
            .summary
            .contains("\"tool_name\":\"tool_search_tool_bm25\""));
    }

    #[test]
    fn tool_search_rejects_empty_query() {
        let error = ToolSearchTool {
            tool_name: "tool_search_tool_bm25",
            mode: ToolSearchMode::Bm25,
        }
        .execute(ToolInput::new().with_arg("query", " "))
        .unwrap_err();

        assert!(error.to_string().contains("requires `query`"));
    }
}
