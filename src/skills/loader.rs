use std::path::Path;

use crate::error::AppResult;
use crate::skills::schema::SkillSpec;

pub fn load_skill(path: &Path) -> AppResult<SkillSpec> {
    let stem = path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("unknown");

    let description = match stem {
        "fix-tests" => "Focus on reproducing and fixing failing tests with minimal edits",
        "fix-lint" => "Focus on lint and typecheck fixes with the smallest safe diff",
        _ => "Local skill placeholder",
    };

    Ok(SkillSpec {
        name: stem.to_string(),
        description: description.to_string(),
        allowed_tools: vec![
            "list_files".to_string(),
            "read_file".to_string(),
            "search_text".to_string(),
            "apply_patch".to_string(),
            "run_shell".to_string(),
            "git_diff".to_string(),
        ],
        system_append: String::new(),
        suggested_steps: vec![],
        policy: crate::skills::schema::SkillPolicy {
            require_write_confirmation: true,
            require_shell_confirmation: false,
            shell_allowlist: vec![],
        },
    })
}
