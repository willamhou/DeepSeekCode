use crate::skills::registry::SkillRegistry;
use crate::skills::schema::SkillSpec;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkillResolution {
    Explicit,
    Auto,
}

#[derive(Debug, Clone, Copy)]
pub struct ResolvedSkill<'a> {
    pub spec: &'a SkillSpec,
    pub resolution: SkillResolution,
}

pub fn resolve_skill<'a>(
    registry: &'a SkillRegistry,
    name: Option<&str>,
    task: &str,
) -> Option<ResolvedSkill<'a>> {
    if let Some(name) = name {
        return registry.find(name).map(|spec| ResolvedSkill {
            spec,
            resolution: SkillResolution::Explicit,
        });
    }

    auto_select_skill(registry, task).map(|spec| ResolvedSkill {
        spec,
        resolution: SkillResolution::Auto,
    })
}

fn auto_select_skill<'a>(registry: &'a SkillRegistry, task: &str) -> Option<&'a SkillSpec> {
    let task_lower = task.to_lowercase();
    let task_requires_patch = task_looks_like_direct_edit(task);
    let mut best: Option<(&SkillSpec, (usize, usize, usize))> = None;

    for skill in registry.iter() {
        if task_requires_patch && !skill_allows_patch(skill) {
            continue;
        }
        if skill.name == "research" && task_looks_like_failure_recovery(&task_lower) {
            continue;
        }
        let score = trigger_score(skill, &task_lower);
        if score == (0, 0, 0) {
            continue;
        }
        match best {
            Some((_, best_score)) if score <= best_score => {}
            _ => best = Some((skill, score)),
        }
    }

    best.map(|(skill, _)| skill)
}

fn task_looks_like_direct_edit(task: &str) -> bool {
    let task_lower = task.to_lowercase();
    task_lower.contains("replace ") && task_lower.contains(" with ") && task_lower.contains(" in ")
}

fn task_looks_like_failure_recovery(task_lower: &str) -> bool {
    [
        "failing test",
        "test fails",
        "test failure",
        "lint failure",
        "build failure",
        "reproduce locally",
        "before retrying",
        "fails in",
    ]
    .iter()
    .any(|needle| task_lower.contains(needle))
}

fn skill_allows_patch(skill: &SkillSpec) -> bool {
    skill.allowed_tools.iter().any(|tool| tool == "apply_patch")
}

fn trigger_score(skill: &SkillSpec, task_lower: &str) -> (usize, usize, usize) {
    let mut matched_count = 0usize;
    let mut total_len = 0usize;
    let mut max_len = 0usize;

    for trigger in &skill.triggers {
        let trigger = trigger.trim().to_lowercase();
        if trigger.is_empty() {
            continue;
        }
        if task_lower.contains(&trigger) {
            matched_count += 1;
            total_len += trigger.len();
            max_len = max_len.max(trigger.len());
        }
    }

    (matched_count, total_len, max_len)
}

#[cfg(test)]
mod tests {
    use super::{resolve_skill, SkillResolution};
    use crate::skills::registry::SkillRegistry;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn unique_test_dir(label: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        std::env::temp_dir().join(format!("dscode_skill_resolver_test_{label}_{nanos}"))
    }

    fn write_skill(dir: &PathBuf, name: &str, body: &str) {
        fs::create_dir_all(dir).unwrap();
        fs::write(dir.join(format!("{name}.toml")), body).unwrap();
    }

    fn base_skill(name: &str, triggers: &[&str]) -> String {
        let trigger_list = if triggers.is_empty() {
            "[]".to_string()
        } else {
            format!(
                "[{}]",
                triggers
                    .iter()
                    .map(|t| format!("\"{t}\""))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        };
        format!(
            r#"name = "{name}"
description = "test"
allowed_tools = ["read_file"]
system_append = "test"
suggested_steps = []
triggers = {trigger_list}

[policy]
require_write_confirmation = false
require_shell_confirmation = false
shell_allowlist = []
"#
        )
    }

    #[test]
    fn explicit_skill_selection_wins_over_auto_select() {
        let dir = unique_test_dir("explicit");
        write_skill(&dir, "research", &base_skill("research", &["research"]));
        write_skill(&dir, "debug", &base_skill("debug", &["bug", "debug"]));
        let (registry, _) = SkillRegistry::load_dirs(&[dir.as_path()]).unwrap();

        let resolved = resolve_skill(&registry, Some("debug"), "research the bug").unwrap();
        assert_eq!(resolved.spec.name, "debug");
        assert_eq!(resolved.resolution, SkillResolution::Explicit);

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn auto_select_uses_best_matching_trigger_set() {
        let dir = unique_test_dir("auto");
        write_skill(
            &dir,
            "research",
            &base_skill("research", &["research", "investigate"]),
        );
        write_skill(
            &dir,
            "write-tests",
            &base_skill("write-tests", &["write tests", "tdd"]),
        );
        let (registry, _) = SkillRegistry::load_dirs(&[dir.as_path()]).unwrap();

        let resolved = resolve_skill(
            &registry,
            None,
            "please write tests for the parser and add missing coverage",
        )
        .unwrap();
        assert_eq!(resolved.spec.name, "write-tests");
        assert_eq!(resolved.resolution, SkillResolution::Auto);

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn auto_select_returns_none_when_no_trigger_matches() {
        let dir = unique_test_dir("none");
        write_skill(&dir, "research", &base_skill("research", &["research"]));
        let (registry, _) = SkillRegistry::load_dirs(&[dir.as_path()]).unwrap();

        let resolved = resolve_skill(&registry, None, "rename foo to bar");
        assert!(resolved.is_none());

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn auto_select_skips_non_writable_skill_for_direct_edit_task() {
        let dir = unique_test_dir("patch-preference");
        write_skill(
            &dir,
            "verify-changes",
            r#"name = "verify-changes"
description = "test"
allowed_tools = ["read_file", "run_shell", "git_diff"]
system_append = "test"
suggested_steps = []
triggers = ["validate"]

[policy]
require_write_confirmation = false
require_shell_confirmation = false
shell_allowlist = []
"#,
        );
        write_skill(
            &dir,
            "debug",
            r#"name = "debug"
description = "test"
allowed_tools = ["read_file", "apply_patch", "run_shell", "git_diff"]
system_append = "test"
suggested_steps = []
triggers = ["replace", "validate"]

[policy]
require_write_confirmation = false
require_shell_confirmation = false
shell_allowlist = []
"#,
        );
        let (registry, _) = SkillRegistry::load_dirs(&[dir.as_path()]).unwrap();

        let resolved = resolve_skill(
            &registry,
            None,
            "replace `a - b` with `a + b` in src/lib.rs and validate with cargo test",
        )
        .unwrap();
        assert_eq!(resolved.spec.name, "debug");

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn auto_select_skips_research_for_failure_recovery_tasks() {
        let dir = unique_test_dir("skip-research-recovery");
        write_skill(
            &dir,
            "research",
            &base_skill("research", &["research", "investigate"]),
        );
        write_skill(
            &dir,
            "debug",
            r#"name = "debug"
description = "test"
allowed_tools = ["read_file", "apply_patch", "run_shell"]
system_append = "test"
suggested_steps = []
triggers = ["fails", "failing test"]

[policy]
require_write_confirmation = false
require_shell_confirmation = false
shell_allowlist = []
"#,
        );
        let (registry, _) = SkillRegistry::load_dirs(&[dir.as_path()]).unwrap();

        let resolved = resolve_skill(
            &registry,
            None,
            "investigate why npm test fails in the JavaScript CLI and inspect the failing test file before retrying",
        )
        .unwrap();
        assert_eq!(resolved.spec.name, "debug");

        let _ = fs::remove_dir_all(dir);
    }
}
