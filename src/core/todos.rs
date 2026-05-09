use std::fmt::Write;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TodoStatus {
    Pending,
    InProgress,
    Completed,
}

impl TodoStatus {
    pub fn from_label(s: &str) -> Option<Self> {
        match s {
            "pending" => Some(Self::Pending),
            "in_progress" => Some(Self::InProgress),
            "completed" => Some(Self::Completed),
            _ => None,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::InProgress => "in_progress",
            Self::Completed => "completed",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Todo {
    pub content: String,
    pub active_form: String,
    pub status: TodoStatus,
}

#[derive(Debug, Clone, Default)]
pub struct TodoList {
    pub items: Vec<Todo>,
}

impl TodoList {
    pub fn replace(&mut self, items: Vec<Todo>) {
        self.items = items;
    }

    pub fn complete_in_progress_matching_subagent_task(&mut self, delegated_task: &str) -> bool {
        let delegated_marker =
            extract_delegated_todo_step(delegated_task).map(normalize_todo_match_key);
        let delegated_fallback = normalize_todo_match_key(delegated_task);
        let Some(index) = self.items.iter().position(|item| {
            matches!(item.status, TodoStatus::InProgress)
                && todo_matches_delegated_task(
                    &item.content,
                    delegated_marker.as_deref(),
                    &delegated_fallback,
                )
        }) else {
            return false;
        };

        self.items[index].status = TodoStatus::Completed;
        if let Some(next) = self
            .items
            .iter_mut()
            .find(|item| matches!(item.status, TodoStatus::Pending))
        {
            next.status = TodoStatus::InProgress;
        }
        true
    }

    /// "- [pending] Run tests\n- [in_progress] Add feature\n…". Empty list → "".
    /// Currently `build_user_prompt` inlines this format directly; kept on
    /// `TodoList` per spec API surface for future reuse.
    #[allow(dead_code)]
    pub fn render_for_prompt(&self) -> String {
        let mut out = String::new();
        for item in &self.items {
            let _ = writeln!(&mut out, "- [{}] {}", item.status.label(), item.content);
        }
        out
    }

    pub fn render_for_display(&self) -> String {
        let mut out = String::new();
        out.push_str(&self.render_compact_summary());
        for item in &self.items {
            let visible = match item.status {
                TodoStatus::InProgress => &item.active_form,
                _ => &item.content,
            };
            let label = format!("[{}]", item.status.label());
            let _ = write!(&mut out, "\n  {label:<14} {visible}");
        }
        out
    }

    pub fn render_compact_summary(&self) -> String {
        if self.items.is_empty() {
            return "no todos".to_string();
        }
        let mut completed = 0usize;
        let mut in_progress = 0usize;
        let mut pending = 0usize;
        for item in &self.items {
            match item.status {
                TodoStatus::Completed => completed += 1,
                TodoStatus::InProgress => in_progress += 1,
                TodoStatus::Pending => pending += 1,
            }
        }
        format!(
            "{total} todos: {completed} completed, {in_progress} in_progress, {pending} pending",
            total = self.items.len(),
        )
    }

    pub fn snapshot(&self) -> Vec<Todo> {
        self.items.clone()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

fn todo_matches_delegated_task(
    todo_content: &str,
    delegated_marker: Option<&str>,
    delegated_fallback: &str,
) -> bool {
    let todo_key = normalize_todo_match_key(todo_content);
    if let Some(marker) = delegated_marker {
        return marker == todo_key;
    }
    delegated_fallback == todo_key
}

fn extract_delegated_todo_step(task: &str) -> Option<&str> {
    let marker = "delegated todo step:";
    let lower = task.to_lowercase();
    let start = lower.find(marker)?;
    let rest = task.get(start + marker.len()..)?.trim_start();
    let rest_lower = rest.to_lowercase();
    let end = rest_lower.find(". parent task:").unwrap_or(rest.len());
    let step = rest.get(..end)?.trim();
    if step.is_empty() {
        None
    } else {
        Some(step)
    }
}

fn normalize_todo_match_key(text: &str) -> String {
    let mut normalized = String::new();
    let mut last_was_space = false;
    for ch in text.chars() {
        let mapped = if ch.is_ascii_alphanumeric() {
            ch.to_ascii_lowercase()
        } else {
            ' '
        };
        if mapped == ' ' {
            if !last_was_space && !normalized.is_empty() {
                normalized.push(' ');
            }
            last_was_space = true;
        } else {
            normalized.push(mapped);
            last_was_space = false;
        }
    }
    normalized.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_label_accepts_three_legal_values() {
        assert_eq!(TodoStatus::from_label("pending"), Some(TodoStatus::Pending));
        assert_eq!(
            TodoStatus::from_label("in_progress"),
            Some(TodoStatus::InProgress)
        );
        assert_eq!(
            TodoStatus::from_label("completed"),
            Some(TodoStatus::Completed)
        );
    }

    #[test]
    fn from_label_returns_none_for_illegal_values() {
        assert_eq!(TodoStatus::from_label("done"), None);
        assert_eq!(TodoStatus::from_label(""), None);
        assert_eq!(TodoStatus::from_label("PENDING"), None);
    }

    #[test]
    fn label_round_trips_with_from_label() {
        for &v in &[
            TodoStatus::Pending,
            TodoStatus::InProgress,
            TodoStatus::Completed,
        ] {
            assert_eq!(TodoStatus::from_label(v.label()), Some(v));
        }
    }

    fn make_todo(c: &str, a: &str, s: TodoStatus) -> Todo {
        Todo {
            content: c.to_string(),
            active_form: a.to_string(),
            status: s,
        }
    }

    #[test]
    fn replace_overwrites_existing_items() {
        let mut list = TodoList::default();
        list.replace(vec![make_todo("X", "Xing", TodoStatus::Pending)]);
        assert_eq!(list.items.len(), 1);
        list.replace(vec![
            make_todo("Y", "Ying", TodoStatus::InProgress),
            make_todo("Z", "Zing", TodoStatus::Completed),
        ]);
        assert_eq!(list.items.len(), 2);
        assert_eq!(list.items[0].content, "Y");
        assert_eq!(list.items[1].content, "Z");
    }

    #[test]
    fn render_for_prompt_uses_status_content_format() {
        let mut list = TodoList::default();
        list.replace(vec![
            make_todo("A", "Aing", TodoStatus::Pending),
            make_todo("B", "Bing", TodoStatus::InProgress),
            make_todo("C", "Cing", TodoStatus::Completed),
        ]);
        let s = list.render_for_prompt();
        assert_eq!(s, "- [pending] A\n- [in_progress] B\n- [completed] C\n");
    }

    #[test]
    fn render_for_display_uses_active_form_for_in_progress_only() {
        let mut list = TodoList::default();
        list.replace(vec![
            make_todo("Run tests", "Running tests", TodoStatus::InProgress),
            make_todo("Refactor", "Refactoring", TodoStatus::Pending),
            make_todo("Read", "Reading", TodoStatus::Completed),
        ]);
        let s = list.render_for_display();
        assert!(
            s.contains("Running tests"),
            "in_progress should use active_form: {s}"
        );
        assert!(
            !s.contains("Refactoring"),
            "pending should NOT use active_form: {s}"
        );
        assert!(s.contains("Refactor"));
        assert!(
            !s.contains("Reading"),
            "completed should NOT use active_form: {s}"
        );
        assert!(s.contains("Read"));
    }

    #[test]
    fn render_compact_summary_counts_each_status() {
        let mut list = TodoList::default();
        list.replace(vec![
            make_todo("A", "Aing", TodoStatus::Completed),
            make_todo("B", "Bing", TodoStatus::Completed),
            make_todo("C", "Cing", TodoStatus::InProgress),
            make_todo("D", "Ding", TodoStatus::Pending),
            make_todo("E", "Eing", TodoStatus::Pending),
        ]);
        let s = list.render_compact_summary();
        assert!(s.contains("5 todos"), "summary: {s}");
        assert!(s.contains("2 completed"), "summary: {s}");
        assert!(s.contains("1 in_progress"), "summary: {s}");
        assert!(s.contains("2 pending"), "summary: {s}");
    }

    #[test]
    fn render_compact_summary_for_empty_list_returns_no_todos() {
        let list = TodoList::default();
        assert_eq!(list.render_compact_summary(), "no todos");
    }

    #[test]
    fn render_for_display_first_line_equals_render_compact_summary() {
        // NEW-1 contract: first line of display == compact summary
        let mut list = TodoList::default();
        list.replace(vec![
            make_todo("X", "Xing", TodoStatus::InProgress),
            make_todo("Y", "Ying", TodoStatus::Pending),
        ]);
        let display = list.render_for_display();
        let summary = list.render_compact_summary();
        let first_line = display.lines().next().unwrap_or("");
        assert_eq!(first_line, summary);
    }

    #[test]
    fn is_empty_tracks_state_changes() {
        let mut list = TodoList::default();
        assert!(list.is_empty());
        list.replace(vec![make_todo("X", "Xing", TodoStatus::Pending)]);
        assert!(!list.is_empty());
        list.replace(vec![]);
        assert!(list.is_empty());
    }

    #[test]
    fn complete_in_progress_matching_subagent_task_advances_next_pending_item() {
        let mut list = TodoList::default();
        list.replace(vec![
            make_todo(
                "Inspect the repository layout",
                "Inspecting the repository layout",
                TodoStatus::InProgress,
            ),
            make_todo(
                "Read the most relevant files",
                "Reading the most relevant files",
                TodoStatus::Pending,
            ),
            make_todo(
                "Implement the requested changes",
                "Implementing the requested changes",
                TodoStatus::Pending,
            ),
        ]);

        let changed = list.complete_in_progress_matching_subagent_task(
            "Delegated todo step: Inspect the repository layout. Parent task: debug the parser",
        );

        assert!(changed);
        assert_eq!(list.items[0].status, TodoStatus::Completed);
        assert_eq!(list.items[1].status, TodoStatus::InProgress);
        assert_eq!(list.items[2].status, TodoStatus::Pending);
    }

    #[test]
    fn complete_in_progress_matching_subagent_task_is_noop_when_task_does_not_match() {
        let mut list = TodoList::default();
        list.replace(vec![
            make_todo(
                "Inspect the repository layout",
                "Inspecting the repository layout",
                TodoStatus::InProgress,
            ),
            make_todo(
                "Read the most relevant files",
                "Reading the most relevant files",
                TodoStatus::Pending,
            ),
        ]);

        let changed =
            list.complete_in_progress_matching_subagent_task("Unrelated subtask for another flow");

        assert!(!changed);
        assert_eq!(list.items[0].status, TodoStatus::InProgress);
        assert_eq!(list.items[1].status, TodoStatus::Pending);
    }

    #[test]
    fn complete_in_progress_matching_subagent_task_uses_explicit_marker_when_present() {
        let mut list = TodoList::default();
        list.replace(vec![
            make_todo(
                "Inspect the repository layout",
                "Inspecting the repository layout",
                TodoStatus::InProgress,
            ),
            make_todo(
                "Read the most relevant files",
                "Reading the most relevant files",
                TodoStatus::Pending,
            ),
        ]);

        let changed = list.complete_in_progress_matching_subagent_task(
            "Delegated todo step: Inspect the repository layout. Parent task: debug the parser. Summarize concrete findings.",
        );

        assert!(changed);
        assert_eq!(list.items[0].status, TodoStatus::Completed);
        assert_eq!(list.items[1].status, TodoStatus::InProgress);
    }

    #[test]
    fn complete_in_progress_matching_subagent_task_does_not_match_similar_but_different_step() {
        let mut list = TodoList::default();
        list.replace(vec![
            make_todo(
                "Inspect the repository layout",
                "Inspecting the repository layout",
                TodoStatus::InProgress,
            ),
            make_todo(
                "Read the most relevant files",
                "Reading the most relevant files",
                TodoStatus::Pending,
            ),
        ]);

        let changed = list.complete_in_progress_matching_subagent_task(
            "Delegated todo step: Inspect the most relevant files. Parent task: debug the parser.",
        );

        assert!(!changed);
        assert_eq!(list.items[0].status, TodoStatus::InProgress);
        assert_eq!(list.items[1].status, TodoStatus::Pending);
    }

    #[test]
    fn complete_in_progress_matching_subagent_task_fallback_requires_exact_normalized_step() {
        let mut list = TodoList::default();
        list.replace(vec![
            make_todo(
                "Inspect repository layout",
                "Inspecting repository layout",
                TodoStatus::InProgress,
            ),
            make_todo(
                "Implement the requested changes",
                "Implementing the requested changes",
                TodoStatus::Pending,
            ),
        ]);

        let changed = list.complete_in_progress_matching_subagent_task("Inspect repository layout");

        assert!(changed);
        assert_eq!(list.items[0].status, TodoStatus::Completed);
        assert_eq!(list.items[1].status, TodoStatus::InProgress);
    }
}
