use std::collections::BTreeMap;

use crate::model::protocol::ObservationStatus;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TurnRole {
    User,
    Assistant,
    Tool,
}

#[derive(Debug, Clone)]
pub struct Turn {
    pub role: TurnRole,
    pub content: String,
    pub tool_name: Option<String>,
    pub tool_input: Option<BTreeMap<String, String>>,
    pub tool_output: Option<String>,
    pub status: ObservationStatus,
}

#[derive(Debug, Clone, Default)]
pub struct Transcript {
    pub turns: Vec<Turn>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompactStats {
    pub before_turns: usize,
    pub after_turns: usize,
    pub summarized_turns: usize,
    pub kept_tail_turns: usize,
}

impl Transcript {
    pub fn push_user(&mut self, content: impl Into<String>) {
        self.turns.push(Turn {
            role: TurnRole::User,
            content: content.into(),
            tool_name: None,
            tool_input: None,
            tool_output: None,
            status: ObservationStatus::Ok,
        });
    }

    pub fn push_assistant(&mut self, content: impl Into<String>) {
        self.turns.push(Turn {
            role: TurnRole::Assistant,
            content: content.into(),
            tool_name: None,
            tool_input: None,
            tool_output: None,
            status: ObservationStatus::Ok,
        });
    }

    pub fn push_tool(
        &mut self,
        name: impl Into<String>,
        input: BTreeMap<String, String>,
        output: impl Into<String>,
        status: ObservationStatus,
    ) {
        self.turns.push(Turn {
            role: TurnRole::Tool,
            content: String::new(),
            tool_name: Some(name.into()),
            tool_input: Some(input),
            tool_output: Some(output.into()),
            status,
        });
    }

    pub fn clear(&mut self) {
        self.turns.clear();
    }
}

use crate::core::observations::summarize_for_kind;
use crate::model::protocol::ObservationKind;

const RECENT_ASSISTANT_TURNS_KEPT_FULL: usize = 3;
const COMPACT_KEEP_TAIL_TURNS: usize = 8;
const COMPACT_SUMMARY_LIMIT: usize = 12 * 1024;
const COMPACT_PREVIEW_CHARS: usize = 180;

impl Transcript {
    pub fn compact(&mut self) -> CompactStats {
        let before_turns = self.turns.len();
        if before_turns <= COMPACT_KEEP_TAIL_TURNS {
            return CompactStats {
                before_turns,
                after_turns: before_turns,
                summarized_turns: 0,
                kept_tail_turns: before_turns,
            };
        }

        let kept_tail_turns = COMPACT_KEEP_TAIL_TURNS;
        let summarized_turns = before_turns - kept_tail_turns;
        let summary = compact_summary(&self.turns[..summarized_turns], kept_tail_turns);
        let mut compacted = Vec::with_capacity(kept_tail_turns + 1);
        compacted.push(Turn {
            role: TurnRole::Assistant,
            content: summary,
            tool_name: None,
            tool_input: None,
            tool_output: None,
            status: ObservationStatus::Ok,
        });
        compacted.extend_from_slice(&self.turns[summarized_turns..]);
        self.turns = compacted;

        CompactStats {
            before_turns,
            after_turns: self.turns.len(),
            summarized_turns,
            kept_tail_turns,
        }
    }

    pub fn render_for_prompt(&self) -> String {
        if self.turns.is_empty() {
            return String::new();
        }
        let assistant_indices: Vec<usize> = self
            .turns
            .iter()
            .enumerate()
            .filter_map(|(i, t)| (t.role == TurnRole::Assistant).then_some(i))
            .collect();
        let keep_full_after = assistant_indices
            .len()
            .saturating_sub(RECENT_ASSISTANT_TURNS_KEPT_FULL);
        let assistants_kept_full: std::collections::BTreeSet<usize> = assistant_indices
            .iter()
            .skip(keep_full_after)
            .copied()
            .collect();

        let mut user_n = 0usize;
        let mut assistant_n = 0usize;
        let mut out = String::from("Conversation so far:\n\n");

        for (i, turn) in self.turns.iter().enumerate() {
            match turn.role {
                TurnRole::User => {
                    user_n += 1;
                    out.push_str(&format!("[user {user_n}]: {}\n\n", turn.content));
                }
                TurnRole::Assistant => {
                    assistant_n += 1;
                    if assistants_kept_full.contains(&i) {
                        out.push_str(&format!("[assistant {assistant_n}]: {}\n\n", turn.content));
                    } else {
                        let head = turn.content.lines().next().unwrap_or("").trim();
                        out.push_str(&format!(
                            "[assistant {assistant_n}]: {head} (truncated assistant turn {assistant_n})\n\n",
                        ));
                    }
                }
                TurnRole::Tool => {
                    let name = turn.tool_name.as_deref().unwrap_or("?");
                    let kind = ObservationKind::from_tool_name(name);
                    let trimmed_output = turn
                        .tool_output
                        .as_ref()
                        .map(|o| summarize_for_kind(o, kind))
                        .unwrap_or_default();
                    let status_label = status_label(turn.status);
                    let input_repr = tool_input_repr(name, turn.tool_input.as_ref());
                    out.push_str(&format!(
                        "[tool] {name}({input_repr}) -> {status_label}\n{trimmed_output}\n\n",
                    ));
                }
            }
        }

        out.push_str("(end of conversation; respond to the latest user message above)\n");
        out
    }
}

fn compact_summary(turns: &[Turn], kept_tail_turns: usize) -> String {
    let mut user_turns = 0usize;
    let mut assistant_turns = 0usize;
    let mut tool_turns = 0usize;
    for turn in turns {
        match turn.role {
            TurnRole::User => user_turns += 1,
            TurnRole::Assistant => assistant_turns += 1,
            TurnRole::Tool => tool_turns += 1,
        }
    }

    let mut out = format!(
        "Compacted conversation summary:\n- Summarized turns: {}\n- Recent turns kept verbatim: {kept_tail_turns}\n- Role counts: user={user_turns}, assistant={assistant_turns}, tool={tool_turns}\n\n",
        turns.len()
    );

    for (index, turn) in turns.iter().enumerate() {
        let turn_number = index + 1;
        let line = match turn.role {
            TurnRole::User => format!(
                "- turn {turn_number} user: {}\n",
                first_line_preview(&turn.content, COMPACT_PREVIEW_CHARS)
            ),
            TurnRole::Assistant => format!(
                "- turn {turn_number} assistant: {}\n",
                first_line_preview(&turn.content, COMPACT_PREVIEW_CHARS)
            ),
            TurnRole::Tool => {
                let name = turn.tool_name.as_deref().unwrap_or("?");
                let kind = ObservationKind::from_tool_name(name);
                let output = turn
                    .tool_output
                    .as_ref()
                    .map(|o| summarize_for_kind(o, kind))
                    .unwrap_or_default();
                format!(
                    "- turn {turn_number} tool: {name}({}) -> {}; output: {}\n",
                    tool_input_repr(name, turn.tool_input.as_ref()),
                    status_label(turn.status),
                    first_line_preview(&output, COMPACT_PREVIEW_CHARS)
                )
            }
        };
        if !push_limited(&mut out, &line, COMPACT_SUMMARY_LIMIT) {
            let _ = push_limited(&mut out, "\n(summary truncated)\n", COMPACT_SUMMARY_LIMIT);
            break;
        }
    }

    out
}

fn first_line_preview(value: &str, max_chars: usize) -> String {
    let line = value
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("")
        .trim();
    if line.is_empty() {
        return "(empty)".to_string();
    }

    let mut preview = String::new();
    for (index, ch) in line.chars().enumerate() {
        if index == max_chars {
            preview.push_str("...");
            return preview;
        }
        preview.push(ch);
    }
    preview
}

fn push_limited(out: &mut String, value: &str, limit: usize) -> bool {
    if out.len() + value.len() <= limit {
        out.push_str(value);
        return true;
    }
    if out.len() >= limit {
        return false;
    }

    let mut end = limit - out.len();
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    out.push_str(&value[..end]);
    false
}

fn status_label(status: ObservationStatus) -> &'static str {
    match status {
        ObservationStatus::Ok => "ok",
        ObservationStatus::Failed => "failed",
    }
}

fn tool_input_repr(name: &str, input: Option<&BTreeMap<String, String>>) -> String {
    if name == "todo_write" {
        return input
            .and_then(|m| m.get("items"))
            .and_then(|s| crate::util::json::parse_json_value(s).ok())
            .and_then(|v| match v {
                crate::util::json::JsonValue::Array(a) => {
                    Some(format!("items=<{} todos>", a.len()))
                }
                _ => None,
            })
            .unwrap_or_else(|| "items=<malformed>".to_string());
    }

    input
        .map(|map| {
            let parts: Vec<String> = map.iter().map(|(k, v)| format!("{k}={v}")).collect();
            parts.join(", ")
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_user_records_a_user_turn() {
        let mut t = Transcript::default();
        t.push_user("hello");
        assert_eq!(t.turns.len(), 1);
        assert_eq!(t.turns[0].role, TurnRole::User);
        assert_eq!(t.turns[0].content, "hello");
    }

    #[test]
    fn push_assistant_records_an_assistant_turn() {
        let mut t = Transcript::default();
        t.push_assistant("done");
        assert_eq!(t.turns.last().unwrap().role, TurnRole::Assistant);
    }

    #[test]
    fn push_tool_records_input_and_output() {
        let mut t = Transcript::default();
        let mut input = BTreeMap::new();
        input.insert("path".to_string(), "x.rs".to_string());
        t.push_tool("read_file", input, "contents", ObservationStatus::Ok);
        let last = t.turns.last().unwrap();
        assert_eq!(last.role, TurnRole::Tool);
        assert_eq!(last.tool_name.as_deref(), Some("read_file"));
        assert_eq!(last.tool_output.as_deref(), Some("contents"));
        assert_eq!(
            last.tool_input
                .as_ref()
                .unwrap()
                .get("path")
                .map(String::as_str),
            Some("x.rs"),
        );
    }

    #[test]
    fn clear_empties_turns() {
        let mut t = Transcript::default();
        t.push_user("a");
        t.push_assistant("b");
        t.clear();
        assert!(t.turns.is_empty());
    }

    #[test]
    fn render_returns_empty_for_empty_transcript() {
        let t = Transcript::default();
        assert!(t.render_for_prompt().is_empty());
    }

    #[test]
    fn render_includes_user_and_assistant_turns() {
        let mut t = Transcript::default();
        t.push_user("ask 1");
        t.push_assistant("answer 1");
        t.push_user("ask 2");
        let rendered = t.render_for_prompt();
        assert!(rendered.contains("[user 1]: ask 1"));
        assert!(rendered.contains("[assistant 1]: answer 1"));
        assert!(rendered.contains("[user 2]: ask 2"));
        assert!(rendered.contains("(end of conversation"));
    }

    #[test]
    fn render_truncates_old_assistant_turns_beyond_three() {
        let mut t = Transcript::default();
        for i in 1..=5 {
            t.push_user(format!("ask {i}"));
            t.push_assistant(format!("long\nbody\nof\nturn\n{i}\nwith\nseveral\nlines"));
        }
        let rendered = t.render_for_prompt();
        assert!(rendered.contains("(truncated assistant turn 1)"));
        assert!(rendered.contains("(truncated assistant turn 2)"));
        assert!(!rendered.contains("(truncated assistant turn 3)"));
        assert!(!rendered.contains("(truncated assistant turn 4)"));
        // The last 3 assistants (3,4,5) keep full body
        assert!(rendered.contains("[assistant 5]: long"));
    }

    #[test]
    fn render_summarises_tool_output_per_kind() {
        let mut t = Transcript::default();
        let mut input = BTreeMap::new();
        input.insert("path".to_string(), "x.rs".to_string());
        let huge: String = (0..200).map(|i| format!("line{i}\n")).collect();
        t.push_tool("read_file", input, huge, ObservationStatus::Ok);
        let rendered = t.render_for_prompt();
        assert!(rendered.contains("[tool] read_file(path=x.rs) -> ok"));
        assert!(rendered.contains("line0"));
        assert!(rendered.contains("truncated"));
    }

    #[test]
    fn render_for_prompt_elides_todo_write_input_to_count() {
        let mut transcript = Transcript::default();
        let mut input = std::collections::BTreeMap::new();
        input.insert(
            "items".to_string(),
            r#"[{"content":"A","activeForm":"Aing","status":"pending"},{"content":"B","activeForm":"Bing","status":"in_progress"}]"#.to_string(),
        );
        transcript.push_tool(
            "todo_write",
            input,
            "2 todos: 0 completed, 1 in_progress, 1 pending",
            crate::model::protocol::ObservationStatus::Ok,
        );
        let render = transcript.render_for_prompt();
        assert!(render.contains("items=<2 todos>"));
        assert!(
            !render.contains(r#""content":"A""#),
            "raw JSON must be elided: {render}"
        );
    }

    #[test]
    fn render_for_prompt_elides_malformed_todo_write_input_as_malformed() {
        let mut transcript = Transcript::default();
        let mut input = std::collections::BTreeMap::new();
        input.insert("items".to_string(), "[not_json".to_string());
        transcript.push_tool(
            "todo_write",
            input,
            "ok",
            crate::model::protocol::ObservationStatus::Ok,
        );
        let render = transcript.render_for_prompt();
        assert!(render.contains("items=<malformed>"));
    }

    #[test]
    fn compact_noops_when_transcript_is_short() {
        let mut transcript = Transcript::default();
        transcript.push_user("short");
        transcript.push_assistant("reply");

        let stats = transcript.compact();

        assert_eq!(
            stats,
            CompactStats {
                before_turns: 2,
                after_turns: 2,
                summarized_turns: 0,
                kept_tail_turns: 2,
            }
        );
        assert_eq!(transcript.turns[0].content, "short");
        assert_eq!(transcript.turns[1].content, "reply");
    }

    #[test]
    fn compact_replaces_old_turns_with_summary_and_keeps_tail() {
        let mut transcript = Transcript::default();
        for index in 0..12 {
            if index == 1 {
                transcript.push_assistant("old first line\nold secret body");
            } else {
                transcript.push_user(format!("turn {index}"));
            }
        }

        let stats = transcript.compact();

        assert_eq!(stats.before_turns, 12);
        assert_eq!(stats.after_turns, 9);
        assert_eq!(stats.summarized_turns, 4);
        assert_eq!(stats.kept_tail_turns, 8);
        assert_eq!(transcript.turns[0].role, TurnRole::Assistant);
        assert!(transcript.turns[0]
            .content
            .contains("Compacted conversation summary"));
        assert!(transcript.turns[0].content.contains("old first line"));
        assert!(!transcript.turns[0].content.contains("old secret body"));
        assert_eq!(transcript.turns[1].content, "turn 4");
        assert_eq!(transcript.turns.last().unwrap().content, "turn 11");
    }

    #[test]
    fn compact_summarises_tool_turns() {
        let mut transcript = Transcript::default();
        for index in 0..4 {
            transcript.push_user(format!("old {index}"));
        }
        let mut input = BTreeMap::new();
        input.insert("path".to_string(), "src/lib.rs".to_string());
        transcript.push_tool(
            "read_file",
            input,
            "line one\nline two\nline three",
            ObservationStatus::Failed,
        );
        for index in 0..8 {
            transcript.push_user(format!("tail {index}"));
        }

        let stats = transcript.compact();

        assert_eq!(stats.summarized_turns, 5);
        let summary = &transcript.turns[0].content;
        assert!(summary.contains("tool: read_file(path=src/lib.rs) -> failed"));
        assert!(summary.contains("line one"));
    }
}
