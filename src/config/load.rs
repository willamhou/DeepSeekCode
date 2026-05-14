use std::fs;
use std::path::Path;

use super::types::AppConfig;
use crate::error::app_error;
use crate::error::AppResult;

pub fn load_or_default() -> AppResult<AppConfig> {
    load_dotenv_if_present()?;
    let mut config = AppConfig::default();
    let path = config.workspace.config_path();

    if Path::new(&path).exists() {
        let content = fs::read_to_string(path)?;
        let selected_profile = first_nonempty_env(&["DSCODE_PROFILE", "DEEPSEEK_PROFILE"])
            .or_else(|| active_profile_from_content(&content));
        parse_config_with_profile(&content, &mut config, selected_profile.as_deref())?;
        if let Some(profile) = selected_profile {
            config.workspace.active_profile = Some(profile);
        }
    }

    apply_env_overrides(&mut config);
    Ok(config)
}

fn load_dotenv_if_present() -> AppResult<()> {
    let path = Path::new(".env");
    if !path.exists() {
        return Ok(());
    }

    let content = fs::read_to_string(path)?;
    for raw_line in content.lines() {
        let Some((key, value)) = parse_dotenv_assignment(raw_line) else {
            continue;
        };
        if std::env::var_os(&key).is_none() {
            std::env::set_var(key, value);
        }
    }
    Ok(())
}

pub(crate) fn parse_dotenv_assignment(raw_line: &str) -> Option<(String, String)> {
    let line = raw_line.trim();
    if line.is_empty() || line.starts_with('#') {
        return None;
    }
    let line = line.strip_prefix("export ").unwrap_or(line).trim();
    let (key, value) = line.split_once('=')?;
    let key = key.trim();
    if key.is_empty()
        || !key
            .chars()
            .all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
        || key.chars().next().is_some_and(|ch| ch.is_ascii_digit())
    {
        return None;
    }
    Some((key.to_string(), unquote(value.trim())))
}

fn apply_env_overrides(config: &mut AppConfig) {
    if let Ok(base_url) = std::env::var("DEEPSEEK_BASE_URL") {
        if !base_url.trim().is_empty() {
            config.model.base_url = base_url;
        }
    }
    if let Ok(model) = std::env::var("DEEPSEEK_MODEL") {
        if !model.trim().is_empty() {
            config.model.model = model;
        }
    }
    if let Ok(api_key_env) = std::env::var("DEEPSEEK_API_KEY_ENV") {
        if !api_key_env.trim().is_empty() {
            config.model.api_key_env = api_key_env;
        }
    }
    if let Ok(reasoning_effort) = std::env::var("DEEPSEEK_REASONING_EFFORT") {
        if !reasoning_effort.trim().is_empty() {
            config.model.reasoning_effort = reasoning_effort;
        }
    }
    if let Some(base_url) =
        first_nonempty_env(&["DSCODE_VISION_BASE_URL", "DEEPSEEK_VISION_BASE_URL"])
    {
        config.vision.base_url = base_url;
    }
    if let Some(model) = first_nonempty_env(&["DSCODE_VISION_MODEL", "DEEPSEEK_VISION_MODEL"]) {
        config.vision.model = model;
    }
    if let Some(api_key_env) =
        first_nonempty_env(&["DSCODE_VISION_API_KEY_ENV", "DEEPSEEK_VISION_API_KEY_ENV"])
    {
        config.vision.api_key_env = api_key_env;
    }
    if let Some(value) = first_nonempty_env(&["DSCODE_MEMORY", "DEEPSEEK_MEMORY"]) {
        config.memory.enabled = parse_env_bool(&value);
    }
    if let Some(value) = first_nonempty_env(&["DSCODE_NOTES_PATH", "DEEPSEEK_NOTES_PATH"]) {
        config.memory.notes_path = value;
    }
    if let Some(value) = first_nonempty_env(&["DSCODE_MEMORY_PATH", "DEEPSEEK_MEMORY_PATH"]) {
        config.memory.memory_path = value;
    }
    if let Some(value) = first_nonempty_env(&["DSCODE_NETWORK_DEFAULT", "DEEPSEEK_NETWORK_DEFAULT"])
    {
        config.network.default = value;
    }
    if let Some(value) = first_nonempty_env(&["DSCODE_NETWORK_ALLOW", "DEEPSEEK_NETWORK_ALLOW"]) {
        config.network.allow = parse_env_list(&value);
    }
    if let Some(value) = first_nonempty_env(&["DSCODE_NETWORK_DENY", "DEEPSEEK_NETWORK_DENY"]) {
        config.network.deny = parse_env_list(&value);
    }
    if let Some(value) = first_nonempty_env(&["DSCODE_NETWORK_AUDIT", "DEEPSEEK_NETWORK_AUDIT"]) {
        config.network.audit = parse_env_bool(&value);
    }
    if let Some(value) =
        first_nonempty_env(&["DSCODE_NETWORK_AUDIT_PATH", "DEEPSEEK_NETWORK_AUDIT_PATH"])
    {
        config.network.audit_path = value;
    }
    if let Some(value) =
        first_nonempty_env(&["DSCODE_SKILLS_REGISTRY_URL", "DEEPSEEK_SKILLS_REGISTRY_URL"])
    {
        config.skills.registry_url = value;
    }
    if let Some(value) =
        first_nonempty_env(&["DSCODE_SKILLS_CACHE_DIR", "DEEPSEEK_SKILLS_CACHE_DIR"])
    {
        config.skills.cache_dir = value;
    }
}

fn first_nonempty_env(keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| {
        std::env::var(key)
            .ok()
            .filter(|value| !value.trim().is_empty())
    })
}

fn parse_env_bool(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "1" | "on" | "true" | "yes" | "y" | "enabled"
    )
}

fn parse_env_list(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .collect()
}

pub(crate) fn config_assignments(content: &str) -> Vec<(String, String)> {
    let mut section = String::new();
    let mut assignments = Vec::new();
    for raw_line in content.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(inner) = line
            .strip_prefix('[')
            .and_then(|value| value.strip_suffix(']'))
        {
            section = inner.trim().to_string();
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        if key.is_empty() {
            continue;
        }
        let full_key = if section.is_empty() {
            key.to_string()
        } else {
            format!("{section}.{key}")
        };
        assignments.push((full_key, value.trim().to_string()));
    }
    assignments
}

fn active_profile_from_content(content: &str) -> Option<String> {
    config_assignments(content)
        .into_iter()
        .find_map(|(key, value)| {
            (key == "workspace.active_profile")
                .then(|| unquote(&value))
                .filter(|profile| !profile.trim().is_empty())
        })
}

#[cfg(test)]
fn parse_config(content: &str, config: &mut AppConfig) -> AppResult<()> {
    parse_config_with_profile(content, config, None)
}

fn parse_config_with_profile(
    content: &str,
    config: &mut AppConfig,
    selected_profile: Option<&str>,
) -> AppResult<()> {
    let assignments = config_assignments(content);
    for (key, value) in assignments
        .iter()
        .filter(|(key, _)| !key.starts_with("profiles."))
    {
        apply_config_key(key, value, config)?;
    }

    if let Some(profile) = selected_profile
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        let mut found = false;
        let prefix = format!("profiles.{profile}.");
        for (key, value) in assignments
            .iter()
            .filter_map(|(key, value)| key.strip_prefix(&prefix).map(|stripped| (stripped, value)))
        {
            found = true;
            apply_config_key(key, value, config)?;
        }
        if !found {
            return Err(app_error(format!("config profile `{profile}` not found")));
        }
        config.workspace.active_profile = Some(profile.to_string());
    }

    Ok(())
}

fn apply_config_key(key: &str, value: &str, config: &mut AppConfig) -> AppResult<()> {
    match key {
        "model.base_url" => config.model.base_url = unquote(value),
        "model.model" => config.model.model = unquote(value),
        "model.api_key_env" => config.model.api_key_env = unquote(value),
        "model.reasoning_effort" => config.model.reasoning_effort = unquote(value),
        "vision.base_url" | "vision_model.base_url" => config.vision.base_url = unquote(value),
        "vision.model" | "vision_model.model" => config.vision.model = unquote(value),
        "vision.api_key_env" | "vision_model.api_key_env" => {
            config.vision.api_key_env = unquote(value)
        }
        "approval.require_write_confirmation" => {
            config.approval.require_write_confirmation = parse_bool(value)?
        }
        "approval.require_shell_confirmation" => {
            config.approval.require_shell_confirmation = parse_bool(value)?
        }
        "approval.require_mcp_confirmation" => {
            config.approval.require_mcp_confirmation = parse_bool(value)?
        }
        "approval.mcp_call_allowlist" => {
            config.approval.mcp_call_allowlist = parse_mcp_call_allowlist(value)?
        }
        "hooks.enabled" => {
            config.hooks.enabled = parse_bool(value)?;
        }
        "hooks.project_dir" => {
            config.hooks.project_dir = unquote(value);
        }
        "hooks.user_dir" => {
            config.hooks.user_dir = unquote(value);
        }
        "hooks.timeout_ms" => {
            config.hooks.timeout_ms = parse_u64(value)?;
        }
        "mcp.enabled" => {
            config.mcp.enabled = parse_bool(value)?;
        }
        "mcp.expose_remote_tools" => {
            config.mcp.expose_remote_tools = parse_bool(value)?;
        }
        "mcp.project_file" => {
            config.mcp.project_file = unquote(value);
        }
        "mcp.user_file" => {
            config.mcp.user_file = unquote(value);
        }
        "diagnostics.post_edit" => {
            config.diagnostics.post_edit = parse_bool(value)?;
        }
        "memory.enabled" => {
            config.memory.enabled = parse_bool(value)?;
        }
        "memory.notes_path" | "notes_path" => {
            config.memory.notes_path = unquote(value);
        }
        "memory.memory_path" | "memory_path" => {
            config.memory.memory_path = unquote(value);
        }
        "network.default" => {
            config.network.default = unquote(value);
        }
        "network.allow" => {
            config.network.allow = parse_string_list(value)?;
        }
        "network.deny" => {
            config.network.deny = parse_string_list(value)?;
        }
        "network.audit" => {
            config.network.audit = parse_bool(value)?;
        }
        "network.audit_path" => {
            config.network.audit_path = unquote(value);
        }
        "skills.registry_url" => {
            config.skills.registry_url = unquote(value);
        }
        "skills.cache_dir" => {
            config.skills.cache_dir = unquote(value);
        }
        "workspace.config_dir" => config.workspace.config_dir = unquote(value),
        "workspace.session_dir" => config.workspace.session_dir = unquote(value),
        "workspace.user_skills_dir" => {
            config.workspace.user_skills_dir = unquote(value);
        }
        "workspace.user_commands_dir" => {
            config.workspace.user_commands_dir = unquote(value);
        }
        "workspace.user_instructions_file" => {
            config.workspace.user_instructions_file = unquote(value);
        }
        "workspace.active_profile" => {
            let profile = unquote(value);
            config.workspace.active_profile = (!profile.trim().is_empty()).then_some(profile);
        }
        _ => {}
    }
    Ok(())
}

fn parse_bool(value: &str) -> AppResult<bool> {
    match value {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => Err(app_error(format!("invalid boolean value: {value}"))),
    }
}

fn parse_u64(value: &str) -> AppResult<u64> {
    value
        .trim_matches('"')
        .parse::<u64>()
        .map_err(|_| app_error(format!("invalid integer value: {value}")))
}

fn parse_mcp_call_allowlist(value: &str) -> AppResult<Vec<String>> {
    let entries = parse_string_list(value)?;
    for entry in &entries {
        validate_mcp_call_pattern(entry)?;
    }
    Ok(entries)
}

fn validate_mcp_call_pattern(value: &str) -> AppResult<()> {
    let Some((server, tool)) = value.split_once('/') else {
        return Err(app_error(format!(
            "invalid MCP call allowlist pattern `{value}`; expected server/tool"
        )));
    };
    if server.trim().is_empty() || tool.trim().is_empty() {
        return Err(app_error(format!(
            "invalid MCP call allowlist pattern `{value}`; expected server/tool"
        )));
    }
    Ok(())
}

fn parse_string_list(value: &str) -> AppResult<Vec<String>> {
    let value = value.trim();
    let Some(inner) = value.strip_prefix('[').and_then(|v| v.strip_suffix(']')) else {
        return Err(app_error(format!("invalid string list value: {value}")));
    };
    let inner = inner.trim();
    if inner.is_empty() {
        return Ok(Vec::new());
    }
    inner
        .split(',')
        .map(|item| {
            let item = item.trim();
            if item.len() < 2 || !item.starts_with('"') || !item.ends_with('"') {
                return Err(app_error(format!("invalid string list item: {item}")));
            }
            Ok(unquote(item))
        })
        .collect()
}

fn unquote(value: &str) -> String {
    value.trim().trim_matches('"').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::types::AppConfig;

    #[test]
    fn default_user_skills_dir_is_xdg_path() {
        let config = AppConfig::default();
        assert_eq!(config.workspace.user_skills_dir, "~/.config/dscode/skills");
    }

    #[test]
    fn parse_config_overrides_user_skills_dir_from_toml() {
        let mut config = AppConfig::default();
        let toml = "workspace.user_skills_dir = \"/custom/skills\"\n";
        parse_config(toml, &mut config).unwrap();
        assert_eq!(config.workspace.user_skills_dir, "/custom/skills");
    }

    #[test]
    fn parse_config_overrides_skills_registry_url_from_toml() {
        let mut config = AppConfig::default();
        let toml = "skills.registry_url = \"https://example.com/skills.json\"\n";
        parse_config(toml, &mut config).unwrap();
        assert_eq!(
            config.skills.registry_url,
            "https://example.com/skills.json"
        );
    }

    #[test]
    fn parse_config_overrides_skills_cache_dir_from_toml() {
        let mut config = AppConfig::default();
        let toml = "skills.cache_dir = \"/tmp/dscode-skills\"\n";
        parse_config(toml, &mut config).unwrap();
        assert_eq!(config.skills.cache_dir, "/tmp/dscode-skills");
    }

    #[test]
    fn parse_config_overrides_user_commands_dir_from_toml() {
        let mut config = AppConfig::default();
        let toml = "workspace.user_commands_dir = \"/custom/commands\"\n";
        parse_config(toml, &mut config).unwrap();
        assert_eq!(config.workspace.user_commands_dir, "/custom/commands");
    }

    #[test]
    fn parse_config_overrides_user_instructions_file_from_toml() {
        let mut config = AppConfig::default();
        let toml = "workspace.user_instructions_file = \"/custom/AGENTS.md\"\n";
        parse_config(toml, &mut config).unwrap();
        assert_eq!(config.workspace.user_instructions_file, "/custom/AGENTS.md");
    }

    #[test]
    fn parse_config_overrides_model_reasoning_effort_from_toml() {
        let mut config = AppConfig::default();
        let toml = "model.reasoning_effort = \"max\"\n";
        parse_config(toml, &mut config).unwrap();
        assert_eq!(config.model.reasoning_effort, "max");
    }

    #[test]
    fn parse_config_accepts_table_sections_and_selected_profiles() {
        let mut config = AppConfig::default();
        let toml = r#"
[model]
model = "base"
reasoning_effort = "off"

[profiles.work]
model.model = "deepseek-v4-pro"
model.reasoning_effort = "max"

[profiles.flash.model]
model = "deepseek-v4-flash"
"#;
        parse_config_with_profile(toml, &mut config, Some("work")).unwrap();

        assert_eq!(config.model.model, "deepseek-v4-pro");
        assert_eq!(config.model.reasoning_effort, "max");
        assert_eq!(config.workspace.active_profile.as_deref(), Some("work"));

        let mut flash = AppConfig::default();
        parse_config_with_profile(toml, &mut flash, Some("flash")).unwrap();
        assert_eq!(flash.model.model, "deepseek-v4-flash");
    }

    #[test]
    fn parse_config_rejects_missing_selected_profile() {
        let mut config = AppConfig::default();
        let error = parse_config_with_profile(
            r#"profiles.work.model.model = "deepseek-v4-pro""#,
            &mut config,
            Some("missing"),
        )
        .unwrap_err();

        assert!(error.to_string().contains("profile `missing` not found"));
    }

    #[test]
    fn parse_config_overrides_vision_from_toml() {
        let mut config = AppConfig::default();
        let toml = r#"
vision.base_url = "https://vision.example/v1"
vision.model = "vision-model"
vision.api_key_env = "VISION_KEY"
"#;
        parse_config(toml, &mut config).unwrap();

        assert_eq!(config.vision.base_url, "https://vision.example/v1");
        assert_eq!(config.vision.model, "vision-model");
        assert_eq!(config.vision.api_key_env, "VISION_KEY");
    }

    #[test]
    fn parse_config_accepts_vision_model_alias_from_toml() {
        let mut config = AppConfig::default();
        let toml = r#"
vision_model.base_url = "https://compat.example/v1"
vision_model.model = "compat-vision-model"
vision_model.api_key_env = "COMPAT_VISION_KEY"
"#;
        parse_config(toml, &mut config).unwrap();

        assert_eq!(config.vision.base_url, "https://compat.example/v1");
        assert_eq!(config.vision.model, "compat-vision-model");
        assert_eq!(config.vision.api_key_env, "COMPAT_VISION_KEY");
    }

    #[test]
    fn parse_config_overrides_approval_from_toml() {
        let mut config = AppConfig::default();
        let toml = r#"
approval.require_write_confirmation = false
approval.require_shell_confirmation = false
approval.require_mcp_confirmation = false
approval.mcp_call_allowlist = ["filesystem/*", "github/list_issues"]
"#;
        parse_config(toml, &mut config).unwrap();

        assert!(!config.approval.require_write_confirmation);
        assert!(!config.approval.require_shell_confirmation);
        assert!(!config.approval.require_mcp_confirmation);
        assert_eq!(
            config.approval.mcp_call_allowlist,
            vec!["filesystem/*", "github/list_issues"]
        );
    }

    #[test]
    fn parse_config_rejects_invalid_mcp_call_allowlist_pattern() {
        let mut config = AppConfig::default();
        let toml = r#"approval.mcp_call_allowlist = ["missing-tool"]"#;
        let error = parse_config(toml, &mut config).unwrap_err();

        assert!(error.to_string().contains("server/tool"));
    }

    #[test]
    fn parse_config_overrides_hooks_from_toml() {
        let mut config = AppConfig::default();
        let toml = r#"
hooks.enabled = true
hooks.project_dir = ".dscode/custom-hooks"
hooks.user_dir = "/custom/user-hooks"
hooks.timeout_ms = 1234
"#;
        parse_config(toml, &mut config).unwrap();

        assert!(config.hooks.enabled);
        assert_eq!(config.hooks.project_dir, ".dscode/custom-hooks");
        assert_eq!(config.hooks.user_dir, "/custom/user-hooks");
        assert_eq!(config.hooks.timeout_ms, 1234);
    }

    #[test]
    fn parse_config_overrides_mcp_from_toml() {
        let mut config = AppConfig::default();
        let toml = r#"
mcp.enabled = false
mcp.expose_remote_tools = true
mcp.project_file = ".dscode/custom-mcp.json"
mcp.user_file = "/custom/user-mcp.json"
"#;
        parse_config(toml, &mut config).unwrap();

        assert!(!config.mcp.enabled);
        assert!(config.mcp.expose_remote_tools);
        assert_eq!(config.mcp.project_file, ".dscode/custom-mcp.json");
        assert_eq!(config.mcp.user_file, "/custom/user-mcp.json");
    }

    #[test]
    fn parse_config_overrides_diagnostics_from_toml() {
        let mut config = AppConfig::default();
        let toml = "diagnostics.post_edit = true\n";
        parse_config(toml, &mut config).unwrap();

        assert!(config.diagnostics.post_edit);
    }

    #[test]
    fn parse_config_overrides_memory_from_toml() {
        let mut config = AppConfig::default();
        let toml = r#"
memory.enabled = true
memory.notes_path = "/custom/notes.md"
memory.memory_path = "/custom/memory.md"
"#;
        parse_config(toml, &mut config).unwrap();

        assert!(config.memory.enabled);
        assert_eq!(config.memory.notes_path, "/custom/notes.md");
        assert_eq!(config.memory.memory_path, "/custom/memory.md");
    }

    #[test]
    fn parse_config_overrides_network_policy_from_toml() {
        let mut config = AppConfig::default();
        let toml = r#"
network.default = "deny"
network.allow = ["api.deepseek.com", ".example.com"]
network.deny = ["tracking.example.com"]
network.audit = false
network.audit_path = "/tmp/dscode-network-audit.log"
"#;
        parse_config(toml, &mut config).unwrap();

        assert_eq!(config.network.default, "deny");
        assert_eq!(
            config.network.allow,
            vec!["api.deepseek.com", ".example.com"]
        );
        assert_eq!(config.network.deny, vec!["tracking.example.com"]);
        assert!(!config.network.audit);
        assert_eq!(config.network.audit_path, "/tmp/dscode-network-audit.log");
    }

    #[test]
    fn parse_dotenv_assignment_accepts_simple_values_and_quotes() {
        assert_eq!(
            parse_dotenv_assignment("DEEPSEEK_MODEL=deepseek-v3.2"),
            Some(("DEEPSEEK_MODEL".to_string(), "deepseek-v3.2".to_string()))
        );
        assert_eq!(
            parse_dotenv_assignment("DEEPSEEK_BASE_URL=\"https://example.test/v1\""),
            Some((
                "DEEPSEEK_BASE_URL".to_string(),
                "https://example.test/v1".to_string()
            ))
        );
    }

    #[test]
    fn parse_dotenv_assignment_accepts_export_prefix() {
        assert_eq!(
            parse_dotenv_assignment("export DEEPSEEK_API_KEY=secret"),
            Some(("DEEPSEEK_API_KEY".to_string(), "secret".to_string()))
        );
    }

    #[test]
    fn parse_dotenv_assignment_rejects_comments_and_bad_keys() {
        assert_eq!(parse_dotenv_assignment("# DEEPSEEK_API_KEY=x"), None);
        assert_eq!(parse_dotenv_assignment("1BAD=x"), None);
        assert_eq!(parse_dotenv_assignment("BAD-NAME=x"), None);
    }
}
