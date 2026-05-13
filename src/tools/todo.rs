use std::cell::RefCell;
use std::rc::Rc;

use crate::core::todos::{Todo, TodoList, TodoStatus};
use crate::error::{tool_failure, AppResult};
use crate::tools::types::{Tool, ToolInput, ToolOutput};
use crate::util::json::{
    json_as_array, json_as_object, json_as_string, parse_json_value, JsonValue,
};

const MAX_ITEMS: usize = 100;

pub struct TodoWriteTool {
    // INVARIANT: this tool's execute() must never call back into the registry while
    // `borrow_mut()` is held. Phase 10b sub-agent dispatch may need to switch to
    // Cell<Vec<Todo>> + take/replace if that invariant changes.
    pub list: Rc<RefCell<TodoList>>,
}

pub struct TodoWriteAliasTool {
    pub list: Rc<RefCell<TodoList>>,
    pub tool_name: &'static str,
}

pub struct UpdatePlanTool {
    pub list: Rc<RefCell<TodoList>>,
}

pub struct TodoAddTool {
    pub list: Rc<RefCell<TodoList>>,
    pub tool_name: &'static str,
}

pub struct TodoUpdateTool {
    pub list: Rc<RefCell<TodoList>>,
    pub tool_name: &'static str,
}

pub struct TodoListTool {
    pub list: Rc<RefCell<TodoList>>,
    pub tool_name: &'static str,
}

impl Tool for TodoWriteTool {
    fn name(&self) -> &str {
        "todo_write"
    }

    fn execute(&self, input: ToolInput) -> AppResult<ToolOutput> {
        let raw_items = input.get("items").ok_or_else(|| {
            tool_failure(
                "todo_write expects an `items` field with a JSON array of \
                 {content, activeForm, status} objects",
            )
        })?;

        let parsed = parse_json_value(raw_items)
            .map_err(|e| tool_failure(format!("malformed todo items JSON: {e}")))?;
        let array = json_as_array(&parsed).ok_or_else(|| {
            tool_failure(format!(
                "`items` must be a JSON array, got {kind}",
                kind = describe_kind(&parsed),
            ))
        })?;

        if array.len() > MAX_ITEMS {
            return Err(tool_failure(format!(
                "too many todos (got {}, max {MAX_ITEMS})",
                array.len()
            )));
        }

        let mut new_items: Vec<Todo> = Vec::with_capacity(array.len());
        for (index, value) in array.iter().enumerate() {
            let obj = json_as_object(value).ok_or_else(|| {
                tool_failure(format!("todo at index {index} must be a JSON object"))
            })?;

            let content = obj
                .get("content")
                .and_then(json_as_string)
                .filter(|s| !s.is_empty())
                .ok_or_else(|| {
                    tool_failure(format!("todo at index {index} missing field `content`"))
                })?
                .to_string();

            let active_form = obj
                .get("activeForm")
                .and_then(json_as_string)
                .filter(|s| !s.is_empty())
                .ok_or_else(|| {
                    tool_failure(format!("todo at index {index} missing field `activeForm`"))
                })?
                .to_string();

            let status_str = obj.get("status").and_then(json_as_string).ok_or_else(|| {
                tool_failure(format!("todo at index {index} missing field `status`"))
            })?;
            let status = TodoStatus::from_label(status_str).ok_or_else(|| {
                tool_failure(format!(
                    "todo at index {index}: status must be pending|in_progress|completed (got `{status_str}`)"
                ))
            })?;

            new_items.push(Todo {
                content,
                active_form,
                status,
            });
        }

        let mut list = self.list.borrow_mut();
        list.replace(new_items);
        let summary = list.render_for_display();
        Ok(ToolOutput { summary })
    }
}

impl Tool for TodoWriteAliasTool {
    fn name(&self) -> &str {
        self.tool_name
    }

    fn execute(&self, input: ToolInput) -> AppResult<ToolOutput> {
        TodoWriteTool {
            list: self.list.clone(),
        }
        .execute(input)
    }
}

impl Tool for UpdatePlanTool {
    fn name(&self) -> &str {
        "update_plan"
    }

    fn execute(&self, input: ToolInput) -> AppResult<ToolOutput> {
        let raw_plan = input.get("plan").ok_or_else(|| {
            tool_failure(
                "update_plan expects a `plan` field with a JSON array of \
                 {step, status} objects",
            )
        })?;
        let parsed = parse_json_value(raw_plan)
            .map_err(|e| tool_failure(format!("malformed update_plan plan JSON: {e}")))?;
        let array = json_as_array(&parsed).ok_or_else(|| {
            tool_failure(format!(
                "`plan` must be a JSON array, got {kind}",
                kind = describe_kind(&parsed),
            ))
        })?;
        if array.len() > MAX_ITEMS {
            return Err(tool_failure(format!(
                "too many plan steps (got {}, max {MAX_ITEMS})",
                array.len()
            )));
        }

        let mut in_progress_seen = false;
        let mut new_items = Vec::with_capacity(array.len());
        for (index, value) in array.iter().enumerate() {
            let obj = json_as_object(value).ok_or_else(|| {
                tool_failure(format!("plan step at index {index} must be a JSON object"))
            })?;
            let step = obj
                .get("step")
                .and_then(json_as_string)
                .filter(|s| !s.is_empty())
                .ok_or_else(|| {
                    tool_failure(format!("plan step at index {index} missing field `step`"))
                })?;
            let status_str = obj
                .get("status")
                .and_then(json_as_string)
                .unwrap_or("pending");
            let mut status = plan_status_from_label(status_str).ok_or_else(|| {
                tool_failure(format!(
                    "plan step at index {index}: status must be pending|in_progress|completed (got `{status_str}`)"
                ))
            })?;
            if matches!(status, TodoStatus::InProgress) {
                if in_progress_seen {
                    status = TodoStatus::Pending;
                } else {
                    in_progress_seen = true;
                }
            }
            new_items.push(Todo {
                content: step.to_string(),
                active_form: format!("Working on {step}"),
                status,
            });
        }

        let mut list = self.list.borrow_mut();
        list.replace(new_items);
        let (pending, in_progress, completed) = todo_counts(&list);
        let progress = plan_progress_percent(&list);
        let mut summary = format!(
            "Plan updated: {pending} pending, {in_progress} in progress, {completed} completed ({progress}% done)"
        );
        if let Some(explanation) = input
            .get("explanation")
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            summary.push_str(&format!("\nExplanation: {explanation}"));
        }
        summary.push('\n');
        summary.push_str(&list.render_for_display());
        Ok(ToolOutput { summary })
    }
}

impl Tool for TodoAddTool {
    fn name(&self) -> &str {
        self.tool_name
    }

    fn execute(&self, input: ToolInput) -> AppResult<ToolOutput> {
        let content = input
            .get("content")
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| tool_failure(format!("{} requires content", self.tool_name)))?;
        let active_form = input
            .get("activeForm")
            .or_else(|| input.get("active_form"))
            .filter(|value| !value.trim().is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| format!("Working on {content}"));
        let status = input
            .get("status")
            .and_then(TodoStatus::from_label)
            .unwrap_or(TodoStatus::Pending);

        let mut list = self.list.borrow_mut();
        if list.items.len() >= MAX_ITEMS {
            return Err(tool_failure(format!(
                "too many todos (got {}, max {MAX_ITEMS})",
                list.items.len()
            )));
        }
        list.items.push(Todo {
            content: content.to_string(),
            active_form,
            status,
        });
        let id = list.items.len();
        let summary = format!("added todo #{id}\n{}", list.render_for_display());
        Ok(ToolOutput { summary })
    }
}

impl Tool for TodoUpdateTool {
    fn name(&self) -> &str {
        self.tool_name
    }

    fn execute(&self, input: ToolInput) -> AppResult<ToolOutput> {
        let id = input
            .get("id")
            .and_then(|value| value.parse::<usize>().ok())
            .filter(|value| *value > 0)
            .ok_or_else(|| tool_failure(format!("{} requires a 1-based id", self.tool_name)))?;
        let status_str = input
            .get("status")
            .ok_or_else(|| tool_failure(format!("{} requires status", self.tool_name)))?;
        let status = TodoStatus::from_label(status_str).ok_or_else(|| {
            tool_failure(format!(
                "{} status must be pending|in_progress|completed",
                self.tool_name
            ))
        })?;

        let mut list = self.list.borrow_mut();
        let Some(item) = list.items.get_mut(id - 1) else {
            return Err(tool_failure(format!("todo id {id} not found")));
        };
        item.status = status;
        let summary = format!(
            "updated todo #{id} to {}\n{}",
            status.label(),
            list.render_for_display()
        );
        Ok(ToolOutput { summary })
    }
}

impl Tool for TodoListTool {
    fn name(&self) -> &str {
        self.tool_name
    }

    fn execute(&self, _input: ToolInput) -> AppResult<ToolOutput> {
        Ok(ToolOutput {
            summary: self.list.borrow().render_for_display(),
        })
    }
}

fn describe_kind(value: &JsonValue) -> &'static str {
    match value {
        JsonValue::Null => "null",
        JsonValue::Bool(_) => "boolean",
        JsonValue::Number(_) => "number",
        JsonValue::String(_) => "string",
        JsonValue::Array(_) => "array",
        JsonValue::Object(_) => "object",
    }
}

fn plan_status_from_label(s: &str) -> Option<TodoStatus> {
    match s {
        "pending" => Some(TodoStatus::Pending),
        "in_progress" | "inprogress" => Some(TodoStatus::InProgress),
        "completed" | "done" => Some(TodoStatus::Completed),
        _ => None,
    }
}

fn todo_counts(list: &TodoList) -> (usize, usize, usize) {
    let mut pending = 0;
    let mut in_progress = 0;
    let mut completed = 0;
    for item in &list.items {
        match item.status {
            TodoStatus::Pending => pending += 1,
            TodoStatus::InProgress => in_progress += 1,
            TodoStatus::Completed => completed += 1,
        }
    }
    (pending, in_progress, completed)
}

fn plan_progress_percent(list: &TodoList) -> usize {
    if list.items.is_empty() {
        return 0;
    }
    let completed = list
        .items
        .iter()
        .filter(|item| matches!(item.status, TodoStatus::Completed))
        .count();
    completed.saturating_mul(100) / list.items.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fresh_list() -> Rc<RefCell<TodoList>> {
        Rc::new(RefCell::new(TodoList::default()))
    }

    fn execute(items_json: &str) -> AppResult<(ToolOutput, Rc<RefCell<TodoList>>)> {
        let list = fresh_list();
        let tool = TodoWriteTool { list: list.clone() };
        let mut input = ToolInput::new();
        input
            .args
            .insert("items".to_string(), items_json.to_string());
        let output = tool.execute(input)?;
        Ok((output, list))
    }

    #[test]
    fn execute_succeeds_with_valid_items_array() {
        let body = r#"[
            {"content":"A","activeForm":"Aing","status":"pending"},
            {"content":"B","activeForm":"Bing","status":"in_progress"}
        ]"#;
        let (output, list) = execute(body).unwrap();
        assert!(output.summary.contains("2 todos"));
        let inner = list.borrow();
        assert_eq!(inner.items.len(), 2);
        assert_eq!(inner.items[0].content, "A");
        assert_eq!(inner.items[1].status, TodoStatus::InProgress);
    }

    #[test]
    fn execute_fails_when_items_missing() {
        let list = fresh_list();
        let tool = TodoWriteTool { list };
        let input = ToolInput::new();
        let err = tool.execute(input).unwrap_err();
        assert!(err.to_string().contains("expects an `items` field"));
    }

    #[test]
    fn execute_fails_when_items_not_valid_json() {
        let err = execute("[not_json").unwrap_err();
        assert!(err.to_string().contains("malformed todo items JSON"));
    }

    #[test]
    fn execute_fails_when_todo_missing_content() {
        let body = r#"[{"activeForm":"Aing","status":"pending"}]"#;
        let err = execute(body).unwrap_err();
        assert!(err.to_string().contains("missing field `content`"));
    }

    #[test]
    fn execute_fails_when_status_invalid() {
        let body = r#"[{"content":"A","activeForm":"Aing","status":"unknown"}]"#;
        let err = execute(body).unwrap_err();
        assert!(err
            .to_string()
            .contains("must be pending|in_progress|completed"));
    }

    #[test]
    fn execute_fails_when_too_many_items() {
        let entry = r#"{"content":"X","activeForm":"Xing","status":"pending"}"#;
        let body = format!("[{}]", vec![entry; 101].join(","));
        let err = execute(&body).unwrap_err();
        assert!(err.to_string().contains("too many todos"));
    }

    #[test]
    fn todo_add_update_and_list_share_state() {
        let list = fresh_list();
        let add = TodoAddTool {
            list: list.clone(),
            tool_name: "todo_add",
        };
        let update = TodoUpdateTool {
            list: list.clone(),
            tool_name: "todo_update",
        };
        let show = TodoListTool {
            list: list.clone(),
            tool_name: "todo_list",
        };

        let added = add
            .execute(ToolInput::new().with_arg("content", "Run tests"))
            .unwrap();
        assert!(added.summary.contains("added todo #1"));

        let updated = update
            .execute(
                ToolInput::new()
                    .with_arg("id", "1")
                    .with_arg("status", "in_progress"),
            )
            .unwrap();
        assert!(updated.summary.contains("updated todo #1 to in_progress"));

        let output = show.execute(ToolInput::new()).unwrap();
        assert!(output.summary.contains("[in_progress]"));
        assert!(output.summary.contains("Working on Run tests"));
    }

    #[test]
    fn checklist_write_alias_reuses_todo_write_validation() {
        let list = fresh_list();
        let tool = TodoWriteAliasTool {
            list,
            tool_name: "checklist_write",
        };

        let output = tool
            .execute(ToolInput::new().with_arg(
                "items",
                r#"[{"content":"Plan","activeForm":"Planning","status":"pending"}]"#,
            ))
            .unwrap();

        assert!(output.summary.contains("1 todos"));
        assert!(output.summary.contains("Plan"));
    }

    #[test]
    fn update_plan_maps_deepseek_tui_steps_to_todos() {
        let list = fresh_list();
        let tool = UpdatePlanTool { list: list.clone() };

        let output = tool
            .execute(
                ToolInput::new()
                    .with_arg("explanation", "Close the next parity gap")
                    .with_arg(
                        "plan",
                        r#"[
                            {"step":"Compare tool surfaces","status":"completed"},
                            {"step":"Implement update_plan alias","status":"in_progress"},
                            {"step":"Run validation","status":"pending"}
                        ]"#,
                    ),
            )
            .unwrap();

        assert!(output.summary.contains("Plan updated"));
        assert!(output.summary.contains("33% done"));
        assert!(output.summary.contains("Close the next parity gap"));
        let inner = list.borrow();
        assert_eq!(inner.items.len(), 3);
        assert_eq!(inner.items[0].content, "Compare tool surfaces");
        assert_eq!(inner.items[0].status, TodoStatus::Completed);
        assert_eq!(
            inner.items[1].active_form,
            "Working on Implement update_plan alias"
        );
        assert_eq!(inner.items[1].status, TodoStatus::InProgress);
    }

    #[test]
    fn update_plan_demotes_duplicate_in_progress_steps() {
        let list = fresh_list();
        let tool = UpdatePlanTool { list: list.clone() };

        tool.execute(ToolInput::new().with_arg(
            "plan",
            r#"[
                {"step":"A","status":"in_progress"},
                {"step":"B","status":"in_progress"}
            ]"#,
        ))
        .unwrap();

        let inner = list.borrow();
        assert_eq!(inner.items[0].status, TodoStatus::InProgress);
        assert_eq!(inner.items[1].status, TodoStatus::Pending);
    }

    #[test]
    fn update_plan_rejects_invalid_plan_shape() {
        let list = fresh_list();
        let tool = UpdatePlanTool { list };

        let err = tool
            .execute(ToolInput::new().with_arg("plan", r#"{"step":"A"}"#))
            .unwrap_err();

        assert!(err.to_string().contains("must be a JSON array"));
    }
}
