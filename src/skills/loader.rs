use std::fs;
use std::path::Path;

use crate::error::AppResult;
use crate::error::app_error;
use crate::skills::schema::{SkillPolicy, SkillSpec};

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
                assign_string(&mut skill, section, key, multiline_buffer.trim_end_matches('\n'))?;
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

        if line.starts_with('[') && line.ends_with(']') {
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

        assign_string(&mut skill, section, &key, &unquote(value))?;
    }

    if multiline.is_some() || array.is_some() {
        return Err(app_error("unterminated skill config structure"));
    }

    Ok(skill)
}

#[derive(Clone, Copy)]
enum Section {
    Root,
    Policy,
}

fn assign_string(skill: &mut SkillSpec, section: Section, key: &str, value: &str) -> AppResult<()> {
    match (section, key) {
        (Section::Root, "name") => skill.name = value.to_string(),
        (Section::Root, "description") => skill.description = value.to_string(),
        (Section::Root, "system_append") => skill.system_append = value.to_string(),
        (Section::Policy, _) => return Err(app_error(format!("unexpected string policy key: {key}"))),
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
        _ => return Err(app_error(format!("unexpected bool key: {key}"))),
    }
    Ok(())
}

fn assign_array(skill: &mut SkillSpec, section: Section, key: &str, value: Vec<String>) -> AppResult<()> {
    match (section, key) {
        (Section::Root, "allowed_tools") => skill.allowed_tools = value,
        (Section::Root, "suggested_steps") => skill.suggested_steps = value,
        (Section::Policy, "shell_allowlist") => skill.policy.shell_allowlist = value,
        _ => return Err(app_error(format!("unexpected array key: {key}"))),
    }
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
}
