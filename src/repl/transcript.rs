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
            last.tool_input.as_ref().unwrap().get("path").map(String::as_str),
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
}
