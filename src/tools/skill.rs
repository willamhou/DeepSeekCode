use std::path::{Path, PathBuf};

use crate::config::types::AppConfig;
use crate::error::{app_error, AppResult};
use crate::skills::loader::load_skill;
use crate::skills::paths::resolve_repo_skills_dir;
use crate::skills::schema::SkillSpec;
use crate::skills::tilde::expand_tilde;
use crate::tools::types::{Tool, ToolInput, ToolOutput};

#[derive(Clone)]
pub struct LoadSkillTool {
    config: AppConfig,
}

impl LoadSkillTool {
    pub fn new(config: AppConfig) -> Self {
        Self { config }
    }
}

impl Tool for LoadSkillTool {
    fn name(&self) -> &str {
        "load_skill"
    }

    fn execute(&self, input: ToolInput) -> AppResult<ToolOutput> {
        let name = input
            .get("name")
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| app_error("load_skill requires `name`"))?;
        let repo_dir = resolve_repo_skills_dir();
        let user_dir = expand_tilde(&self.config.workspace.user_skills_dir);
        let loaded = find_skill_in_dirs(name, &[repo_dir.as_path(), user_dir.as_path()])?;
        let Some((skill, path)) = loaded.skill else {
            let available = if loaded.available.is_empty() {
                "none".to_string()
            } else {
                loaded.available.join(", ")
            };
            return Err(app_error(format!(
                "skill `{name}` not found. Available: {available}. Searched: {}",
                loaded
                    .searched
                    .iter()
                    .map(|path| path.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            )));
        };
        Ok(ToolOutput {
            summary: render_skill_context(&skill, &path),
        })
    }
}

#[derive(Default)]
struct SkillLookup {
    skill: Option<(SkillSpec, PathBuf)>,
    available: Vec<String>,
    searched: Vec<PathBuf>,
}

fn find_skill_in_dirs(name: &str, dirs: &[&Path]) -> AppResult<SkillLookup> {
    let mut lookup = SkillLookup::default();
    for dir in dirs {
        lookup.searched.push((*dir).to_path_buf());
        if !dir.exists() {
            continue;
        }
        let mut entries = std::fs::read_dir(dir)?.collect::<Result<Vec<_>, _>>()?;
        entries.sort_by_key(|entry| entry.path());
        for entry in entries {
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("toml") {
                continue;
            }
            let skill = load_skill(&path)?;
            if !lookup
                .available
                .iter()
                .any(|existing| existing == &skill.name)
            {
                lookup.available.push(skill.name.clone());
            }
            if skill.name == name {
                lookup.skill = Some((skill, path));
            }
        }
    }
    lookup.available.sort();
    Ok(lookup)
}

fn render_skill_context(skill: &SkillSpec, path: &Path) -> String {
    let mut out = String::new();
    out.push_str(&format!("# Skill: {}\n\n", skill.name));
    if !skill.description.trim().is_empty() {
        out.push_str(&format!("Description: {}\n\n", skill.description.trim()));
    }
    out.push_str(&format!("Source: `{}`\n\n", path.display()));
    push_list(&mut out, "Allowed tools", &skill.allowed_tools);
    push_list(&mut out, "Triggers", &skill.triggers);
    push_list(&mut out, "Suggested steps", &skill.suggested_steps);
    if !skill.initial_todos.is_empty() {
        out.push_str("## Initial todos\n\n");
        for todo in &skill.initial_todos {
            out.push_str(&format!(
                "- [{}] {} (active: {})\n",
                todo.status.label(),
                todo.content,
                todo.active_form
            ));
        }
        out.push('\n');
    }
    push_list(&mut out, "References", &skill.references);
    out.push_str("## Policy\n\n");
    out.push_str(&format!(
        "- require_write_confirmation: {}\n",
        skill.policy.require_write_confirmation
    ));
    out.push_str(&format!(
        "- require_shell_confirmation: {}\n",
        skill.policy.require_shell_confirmation
    ));
    push_list(&mut out, "Shell allowlist", &skill.policy.shell_allowlist);
    if !skill.system_append.trim().is_empty() {
        out.push_str("## System Append\n\n");
        out.push_str(skill.system_append.trim());
        out.push('\n');
    }
    out
}

fn push_list(out: &mut String, title: &str, items: &[String]) {
    if items.is_empty() {
        return;
    }
    out.push_str(&format!("## {title}\n\n"));
    for item in items {
        out.push_str(&format!("- {item}\n"));
    }
    out.push('\n');
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_root(label: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "deepseek-load-skill-{label}-{}-{nanos}",
            std::process::id()
        ));
        fs::create_dir_all(&root).unwrap();
        root
    }

    fn write_skill(dir: &Path, file: &str, name: &str, description: &str, system_append: &str) {
        fs::create_dir_all(dir).unwrap();
        fs::write(
            dir.join(file),
            format!(
                r#"name = "{name}"
description = "{description}"
allowed_tools = ["read_file", "grep_files"]
system_append = "{system_append}"
suggested_steps = ["Read context", "Respond"]
triggers = ["{name}"]
references = ["docs/runtime.md"]

[[initial_todos]]
content = "Read context"
active_form = "Reading context"
status = "pending"

[policy]
require_write_confirmation = true
require_shell_confirmation = true
shell_allowlist = ["cargo test"]
"#
            ),
        )
        .unwrap();
    }

    #[test]
    fn load_skill_renders_toml_skill_context() {
        let repo = temp_root("repo");
        let missing_user = temp_root("missing-user").join("none");
        write_skill(&repo, "debug.toml", "debug", "Debug code", "Stay focused.");
        let loaded = find_skill_in_dirs("debug", &[repo.as_path(), missing_user.as_path()])
            .unwrap()
            .skill
            .unwrap();
        let rendered = render_skill_context(&loaded.0, &loaded.1);

        assert!(rendered.contains("# Skill: debug"));
        assert!(rendered.contains("Description: Debug code"));
        assert!(rendered.contains("## Allowed tools"));
        assert!(rendered.contains("## Initial todos"));
        assert!(rendered.contains("Stay focused."));
    }

    #[test]
    fn load_skill_user_dir_overrides_repo_dir() {
        let repo = temp_root("repo-override");
        let user = temp_root("user-override");
        write_skill(&repo, "debug.toml", "debug", "Repo debug", "Repo body.");
        write_skill(&user, "debug.toml", "debug", "User debug", "User body.");

        let loaded = find_skill_in_dirs("debug", &[repo.as_path(), user.as_path()])
            .unwrap()
            .skill
            .unwrap();
        let rendered = render_skill_context(&loaded.0, &loaded.1);

        assert!(rendered.contains("Description: User debug"));
        assert!(rendered.contains("User body."));
    }

    #[test]
    fn load_skill_missing_reports_available_names() {
        let repo = temp_root("missing");
        write_skill(&repo, "debug.toml", "debug", "Debug code", "Body.");

        let loaded = find_skill_in_dirs("nope", &[repo.as_path()]).unwrap();

        assert!(loaded.skill.is_none());
        assert_eq!(loaded.available, vec!["debug".to_string()]);
    }

    #[test]
    fn load_skill_tool_uses_configured_user_skill_dir() {
        let user = temp_root("configured-user");
        write_skill(
            &user,
            "local.toml",
            "local",
            "Local skill",
            "Loaded from user dir.",
        );
        let mut config = AppConfig::default();
        config.workspace.user_skills_dir = user.display().to_string();

        let output = LoadSkillTool::new(config)
            .execute(ToolInput::new().with_arg("name", "local"))
            .unwrap();

        assert!(output.summary.contains("# Skill: local"));
        assert!(output.summary.contains("Loaded from user dir."));
    }
}
