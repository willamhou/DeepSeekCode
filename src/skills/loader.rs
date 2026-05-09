use std::fs;
use std::path::Path;

use crate::core::todos::TodoStatus;
use crate::error::app_error;
use crate::error::AppResult;
use crate::skills::schema::{SkillPolicy, SkillSpec, TodoSeed};

pub fn load_skill(path: &Path) -> AppResult<SkillSpec> {
    let stem = path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("unknown");
    let content = fs::read_to_string(path)?;
    parse_skill_toml(&content, stem)
}

fn parse_skill_toml(content: &str, fallback_name: &str) -> AppResult<SkillSpec> {
    let mut skill = SkillSpec {
        name: fallback_name.to_string(),
        description: String::new(),
        allowed_tools: Vec::new(),
        system_append: String::new(),
        suggested_steps: Vec::new(),
        triggers: Vec::new(),
        initial_todos: Vec::new(),
        references: Vec::new(),
        policy: SkillPolicy {
            require_write_confirmation: true,
            require_shell_confirmation: false,
            shell_allowlist: Vec::new(),
        },
    };

    let mut section = Section::Root;
    let mut multiline = None::<String>;
    let mut multiline_buffer = String::new();
    let mut array = None::<(Section, String, Vec<String>)>;
    let mut current_todo_seed = None::<TodoSeed>;

    for raw_line in content.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        if let Some(key) = multiline.as_ref() {
            if let Some(end_index) = line.find("\"\"\"") {
                let chunk = &line[..end_index];
                if !chunk.is_empty() {
                    multiline_buffer.push_str(chunk);
                }
                assign_string(
                    &mut skill,
                    section,
                    key,
                    multiline_buffer.trim_end_matches('\n'),
                )?;
                multiline = None;
                multiline_buffer.clear();
            } else {
                multiline_buffer.push_str(line);
                multiline_buffer.push('\n');
            }
            continue;
        }

        if let Some((array_section, key, values)) = array.as_mut() {
            if line.starts_with(']') {
                assign_array(&mut skill, *array_section, key, values.clone())?;
                array = None;
            } else if let Some(value) = parse_array_item(line) {
                values.push(value);
            }
            continue;
        }

        if line.starts_with("[[") && line.ends_with("]]") {
            flush_current_todo_seed(&mut skill, &mut current_todo_seed)?;
            section = match &line[2..line.len() - 2] {
                "initial_todos" => {
                    current_todo_seed = Some(TodoSeed {
                        content: String::new(),
                        active_form: String::new(),
                        status: TodoStatus::Pending,
                    });
                    Section::InitialTodo
                }
                _ => Section::Root,
            };
            continue;
        }

        if line.starts_with('[') && line.ends_with(']') {
            flush_current_todo_seed(&mut skill, &mut current_todo_seed)?;
            section = match &line[1..line.len() - 1] {
                "policy" => Section::Policy,
                _ => Section::Root,
            };
            continue;
        }

        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim().to_string();
        let value = value.trim();

        if matches!(section, Section::InitialTodo) && !is_initial_todo_key(&key) {
            flush_current_todo_seed(&mut skill, &mut current_todo_seed)?;
            section = Section::Root;
        }

        if value.starts_with("\"\"\"") {
            let remainder = value.trim_start_matches("\"\"\"");
            if let Some(end_index) = remainder.find("\"\"\"") {
                assign_string(&mut skill, section, &key, &remainder[..end_index])?;
            } else {
                multiline = Some(key);
                if !remainder.is_empty() {
                    multiline_buffer.push_str(remainder);
                    multiline_buffer.push('\n');
                }
            }
            continue;
        }

        if value.starts_with('[') {
            if value.ends_with(']') {
                assign_array(&mut skill, section, &key, parse_inline_array(value)?)?;
            } else {
                array = Some((section, key, Vec::new()));
            }
            continue;
        }

        if value == "true" || value == "false" {
            assign_bool(&mut skill, section, &key, value == "true")?;
            continue;
        }

        if matches!(section, Section::InitialTodo) {
            let todo = current_todo_seed.as_mut().ok_or_else(|| {
                app_error(format!(
                    "encountered initial_todos field `{key}` without an active [[initial_todos]] block"
                ))
            })?;
            assign_initial_todo_string(todo, &key, &unquote(value))?;
            continue;
        }

        assign_string(&mut skill, section, &key, &unquote(value))?;
    }

    if multiline.is_some() || array.is_some() {
        return Err(app_error("unterminated skill config structure"));
    }
    flush_current_todo_seed(&mut skill, &mut current_todo_seed)?;

    Ok(skill)
}

#[derive(Clone, Copy)]
enum Section {
    Root,
    Policy,
    InitialTodo,
}

fn assign_string(skill: &mut SkillSpec, section: Section, key: &str, value: &str) -> AppResult<()> {
    match (section, key) {
        (Section::Root, "name") => skill.name = value.to_string(),
        (Section::Root, "description") => skill.description = value.to_string(),
        (Section::Root, "system_append") => skill.system_append = value.to_string(),
        (Section::InitialTodo, _) => unreachable!("initial_todos handled before assign_string"),
        (Section::Policy, _) => {
            return Err(app_error(format!("unexpected string policy key: {key}")))
        }
        _ => {}
    }
    Ok(())
}

fn assign_bool(skill: &mut SkillSpec, section: Section, key: &str, value: bool) -> AppResult<()> {
    match (section, key) {
        (Section::Policy, "require_write_confirmation") => {
            skill.policy.require_write_confirmation = value;
        }
        (Section::Policy, "require_shell_confirmation") => {
            skill.policy.require_shell_confirmation = value;
        }
        (Section::InitialTodo, _) => {
            return Err(app_error(format!(
                "unexpected bool key in initial_todos: {key}"
            )));
        }
        _ => return Err(app_error(format!("unexpected bool key: {key}"))),
    }
    Ok(())
}

fn assign_array(
    skill: &mut SkillSpec,
    section: Section,
    key: &str,
    value: Vec<String>,
) -> AppResult<()> {
    match (section, key) {
        (Section::Root, "allowed_tools") => skill.allowed_tools = value,
        (Section::Root, "suggested_steps") => skill.suggested_steps = value,
        (Section::Root, "triggers") => skill.triggers = value,
        (Section::Root, "references") => skill.references = value,
        (Section::Policy, "shell_allowlist") => skill.policy.shell_allowlist = value,
        (Section::InitialTodo, _) => {
            return Err(app_error(format!(
                "unexpected array key in initial_todos: {key}"
            )));
        }
        _ => return Err(app_error(format!("unexpected array key: {key}"))),
    }
    Ok(())
}

fn assign_initial_todo_string(todo: &mut TodoSeed, key: &str, value: &str) -> AppResult<()> {
    match key {
        "content" => todo.content = value.to_string(),
        "active_form" | "activeForm" => todo.active_form = value.to_string(),
        "status" => {
            todo.status = TodoStatus::from_label(value).ok_or_else(|| {
                app_error(format!(
                    "initial_todos status must be pending|in_progress|completed (got `{value}`)"
                ))
            })?;
        }
        _ => return Err(app_error(format!("unexpected initial_todos key: {key}"))),
    }
    Ok(())
}

fn is_initial_todo_key(key: &str) -> bool {
    matches!(key, "content" | "active_form" | "activeForm" | "status")
}

fn flush_current_todo_seed(
    skill: &mut SkillSpec,
    current_todo_seed: &mut Option<TodoSeed>,
) -> AppResult<()> {
    let Some(todo) = current_todo_seed.take() else {
        return Ok(());
    };

    if todo.content.trim().is_empty() {
        return Err(app_error(
            "initial_todos entry missing required field `content`",
        ));
    }
    if todo.active_form.trim().is_empty() {
        return Err(app_error(
            "initial_todos entry missing required field `active_form`",
        ));
    }

    skill.initial_todos.push(todo);
    Ok(())
}

fn parse_inline_array(value: &str) -> AppResult<Vec<String>> {
    let inner = value
        .trim()
        .trim_start_matches('[')
        .trim_end_matches(']')
        .trim();
    if inner.is_empty() {
        return Ok(Vec::new());
    }

    let mut items = Vec::new();
    for part in inner.split(',') {
        let part = part.trim();
        if !part.is_empty() {
            items.push(unquote(part));
        }
    }
    Ok(items)
}

fn parse_array_item(line: &str) -> Option<String> {
    let value = line.trim().trim_end_matches(',').trim();
    if value.is_empty() {
        None
    } else {
        Some(unquote(value))
    }
}

fn unquote(value: &str) -> String {
    value.trim().trim_matches('"').to_string()
}

#[cfg(test)]
mod tests {
    use super::parse_skill_toml;
    use crate::core::todos::TodoStatus;

    #[test]
    fn parses_skill_file_shape() {
        let content = r#"
name = "fix-tests"
description = "Focus on reproducing and fixing failing tests with minimal edits"
allowed_tools = ["list_files", "read_file"]
system_append = """
Reproduce failures first.
Rerun relevant tests.
"""
suggested_steps = [
  "Find the test command",
  "Reproduce the failure"
]

[policy]
require_write_confirmation = true
require_shell_confirmation = false
shell_allowlist = ["cargo test", "pytest"]
"#;

        let skill = parse_skill_toml(content, "fallback").unwrap();
        assert_eq!(skill.name, "fix-tests");
        assert_eq!(skill.allowed_tools.len(), 2);
        assert_eq!(skill.suggested_steps.len(), 2);
        assert!(skill.system_append.contains("Reproduce failures first."));
        assert_eq!(skill.policy.shell_allowlist, vec!["cargo test", "pytest"]);
        assert!(skill.policy.require_write_confirmation);
        assert!(!skill.policy.require_shell_confirmation);
    }

    #[test]
    fn parses_schema_v2_fields_when_present() {
        let content = r#"
name = "research"
description = "Research things"
allowed_tools = ["todo_write", "run_shell"]
system_append = "Do research"
suggested_steps = ["Plan", "Search"]
triggers = ["research", "investigate"]
references = ["docs/research-playbook.md", "https://example.com/spec"]

[[initial_todos]]
content = "Plan the research pass"
active_form = "Planning the research pass"
status = "in_progress"

[[initial_todos]]
content = "Run the first search"
activeForm = "Running the first search"
status = "pending"

[policy]
require_write_confirmation = false
require_shell_confirmation = false
shell_allowlist = ["gh search", "curl -sSL"]
"#;

        let skill = parse_skill_toml(content, "fallback").unwrap();
        assert_eq!(skill.triggers, vec!["research", "investigate"]);
        assert_eq!(
            skill.references,
            vec!["docs/research-playbook.md", "https://example.com/spec"]
        );
        assert_eq!(skill.initial_todos.len(), 2);
        assert_eq!(skill.initial_todos[0].content, "Plan the research pass");
        assert_eq!(skill.initial_todos[0].status, TodoStatus::InProgress);
        assert_eq!(
            skill.initial_todos[1].active_form,
            "Running the first search"
        );
        assert_eq!(skill.initial_todos[1].status, TodoStatus::Pending);
    }

    #[test]
    fn initial_todos_require_content_and_active_form() {
        let missing_content = r#"
[[initial_todos]]
active_form = "Planning"
"#;
        let err = parse_skill_toml(missing_content, "fallback").unwrap_err();
        assert!(err.to_string().contains("missing required field `content`"));

        let missing_active_form = r#"
[[initial_todos]]
content = "Plan"
"#;
        let err = parse_skill_toml(missing_active_form, "fallback").unwrap_err();
        assert!(err
            .to_string()
            .contains("missing required field `active_form`"));
    }
}
