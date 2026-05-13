use crate::cli::app::ConfigArgs;
use crate::config::load::load_or_default;
use crate::config::types::AppConfig;
use crate::core::network_policy::{decide, normalize_host, NetworkDecision};
use crate::error::app_error;
use crate::error::AppResult;

pub fn run(args: ConfigArgs) -> AppResult<()> {
    if let Some(host) = args.network_allow {
        let result =
            persist_network_rule_at(&std::env::current_dir()?, &host, NetworkRuleTarget::Allow)?;
        print_network_rule_result(&result);
        return Ok(());
    }
    if let Some(host) = args.network_deny {
        let result =
            persist_network_rule_at(&std::env::current_dir()?, &host, NetworkRuleTarget::Deny)?;
        print_network_rule_result(&result);
        return Ok(());
    }

    if args.init {
        let path = init_config_at(&std::env::current_dir()?, args.force)?;
        println!("initialized config: {}", path.display());
        return Ok(());
    }

    let config = load_or_default()?;
    if args.print_default {
        print_config(&config);
    } else {
        println!(
            "Config file path: {}",
            config.workspace.config_path().display()
        );
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NetworkRuleTarget {
    Allow,
    Deny,
}

impl NetworkRuleTarget {
    fn key(self) -> &'static str {
        match self {
            Self::Allow => "network.allow",
            Self::Deny => "network.deny",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct NetworkRuleResult {
    path: std::path::PathBuf,
    key: &'static str,
    host: String,
    changed: bool,
}

fn print_network_rule_result(result: &NetworkRuleResult) {
    if result.changed {
        println!("{}: added {}", result.key, result.host);
    } else {
        println!("{}: {} already present", result.key, result.host);
    }
    println!("config: {}", result.path.display());
}

fn print_config(config: &AppConfig) {
    println!("model.base_url = {}", config.model.base_url);
    println!("model.model = {}", config.model.model);
    println!("model.api_key_env = {}", config.model.api_key_env);
    println!("model.reasoning_effort = {}", config.model.reasoning_effort);
    println!("vision.base_url = {}", config.vision.base_url);
    println!("vision.model = {}", config.vision.model);
    println!("vision.api_key_env = {}", config.vision.api_key_env);
    println!(
        "approval.require_write_confirmation = {}",
        config.approval.require_write_confirmation
    );
    println!(
        "approval.require_shell_confirmation = {}",
        config.approval.require_shell_confirmation
    );
    println!(
        "approval.require_mcp_confirmation = {}",
        config.approval.require_mcp_confirmation
    );
    println!(
        "approval.mcp_call_allowlist = {}",
        render_string_list(&config.approval.mcp_call_allowlist)
    );
    println!("workspace.config_dir = {}", config.workspace.config_dir);
    println!("workspace.session_dir = {}", config.workspace.session_dir);
    println!(
        "workspace.user_skills_dir = {}",
        config.workspace.user_skills_dir
    );
    println!(
        "workspace.user_commands_dir = {}",
        config.workspace.user_commands_dir
    );
    println!(
        "workspace.user_instructions_file = {}",
        config.workspace.user_instructions_file
    );
    println!("hooks.enabled = {}", config.hooks.enabled);
    println!("hooks.project_dir = {}", config.hooks.project_dir);
    println!("hooks.user_dir = {}", config.hooks.user_dir);
    println!("hooks.timeout_ms = {}", config.hooks.timeout_ms);
    println!("mcp.enabled = {}", config.mcp.enabled);
    println!(
        "mcp.expose_remote_tools = {}",
        config.mcp.expose_remote_tools
    );
    println!("mcp.project_file = {}", config.mcp.project_file);
    println!("mcp.user_file = {}", config.mcp.user_file);
    println!("diagnostics.post_edit = {}", config.diagnostics.post_edit);
    println!("memory.enabled = {}", config.memory.enabled);
    println!("memory.notes_path = {}", config.memory.notes_path);
    println!("memory.memory_path = {}", config.memory.memory_path);
    println!("network.default = {}", config.network.default);
    println!(
        "network.allow = {}",
        render_string_list(&config.network.allow)
    );
    println!(
        "network.deny = {}",
        render_string_list(&config.network.deny)
    );
    println!("network.audit = {}", config.network.audit);
    println!("network.audit_path = {}", config.network.audit_path);
}

pub(crate) fn init_config_at(root: &std::path::Path, force: bool) -> AppResult<std::path::PathBuf> {
    let config = AppConfig::default();
    let config_dir = root.join(&config.workspace.config_dir);
    let config_path = config_dir.join("config.toml");

    if config_path.exists() && !force {
        return Err(app_error(format!(
            "config already exists: {} (use --force to overwrite)",
            config_path.display()
        )));
    }

    std::fs::create_dir_all(&config_dir)?;
    std::fs::write(&config_path, render_default_config(&config))?;
    std::fs::create_dir_all(root.join(&config.workspace.session_dir))?;
    std::fs::create_dir_all(root.join(&config.workspace.config_dir).join("commands"))?;
    std::fs::create_dir_all(root.join(&config.workspace.config_dir).join("agents"))?;

    for event in [
        "session_start",
        "session_stop",
        "user_prompt_submit",
        "pre_tool_use",
        "permission_request",
        "post_tool_use",
        "subagent_start",
        "subagent_stop",
        "pre_compact",
        "shell_env",
    ] {
        std::fs::create_dir_all(root.join(&config.hooks.project_dir).join(event))?;
    }
    let mcp_path = root.join(config.mcp.project_file_path());
    if !mcp_path.exists() {
        if let Some(parent) = mcp_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&mcp_path, render_default_mcp_config())?;
    }

    Ok(config_path)
}

fn persist_network_rule_at(
    root: &std::path::Path,
    host: &str,
    target: NetworkRuleTarget,
) -> AppResult<NetworkRuleResult> {
    let host = normalize_network_rule(host)?;
    let default_config = AppConfig::default();
    let path = root.join(default_config.workspace.config_path());
    if !path.exists() {
        init_config_at(root, false)?;
    }

    let content = std::fs::read_to_string(&path)?;
    let mut allow = read_string_list_key(&content, "network.allow");
    let mut deny = read_string_list_key(&content, "network.deny");
    let changed = match target {
        NetworkRuleTarget::Allow => insert_sorted_unique(&mut allow, &host),
        NetworkRuleTarget::Deny => insert_sorted_unique(&mut deny, &host),
    };

    if target == NetworkRuleTarget::Allow {
        let mut future = crate::config::types::NetworkConfig::default();
        future.allow = allow.clone();
        future.deny = deny.clone();
        if decide(&future, &host) == NetworkDecision::Deny {
            return Err(app_error(format!(
                "network.deny already matches `{host}`; remove the deny rule before adding an allow rule"
            )));
        }
    }

    if changed {
        let values = match target {
            NetworkRuleTarget::Allow => &allow,
            NetworkRuleTarget::Deny => &deny,
        };
        let updated = replace_or_append_string_list_key(&content, target.key(), values);
        std::fs::write(&path, updated)?;
    }

    Ok(NetworkRuleResult {
        path,
        key: target.key(),
        host,
        changed,
    })
}

fn insert_sorted_unique(values: &mut Vec<String>, value: &str) -> bool {
    if values.iter().any(|existing| existing == value) {
        return false;
    }
    values.push(value.to_string());
    values.sort();
    true
}

fn normalize_network_rule(host: &str) -> AppResult<String> {
    let normalized = normalize_host(host);
    if normalized.is_empty()
        || normalized.contains('/')
        || normalized.contains('\\')
        || normalized.contains(',')
        || normalized.contains('"')
        || normalized.contains('\'')
        || normalized.chars().any(char::is_whitespace)
    {
        return Err(app_error(
            "network host rule must be a host, .subdomain suffix, or *.subdomain suffix",
        ));
    }
    Ok(normalized)
}

fn read_string_list_key(content: &str, key: &str) -> Vec<String> {
    for line in content.lines() {
        let trimmed = line.trim_start();
        let Some(rest) = trimmed.strip_prefix(key) else {
            continue;
        };
        let rest = rest.trim_start();
        let Some(value) = rest.strip_prefix('=') else {
            continue;
        };
        return parse_string_list_literal(value.trim());
    }
    Vec::new()
}

fn parse_string_list_literal(value: &str) -> Vec<String> {
    let Some(start) = value.find('[') else {
        return Vec::new();
    };
    let Some(end) = value[start + 1..].find(']') else {
        return Vec::new();
    };
    let body = &value[start + 1..start + 1 + end];
    let mut values = Vec::new();
    let mut current = String::new();
    let mut in_string = false;
    let mut escaped = false;
    for ch in body.chars() {
        if !in_string {
            if ch == '"' {
                in_string = true;
                current.clear();
            }
            continue;
        }
        if escaped {
            current.push(ch);
            escaped = false;
            continue;
        }
        match ch {
            '\\' => escaped = true,
            '"' => {
                in_string = false;
                values.push(current.clone());
            }
            _ => current.push(ch),
        }
    }
    values
}

fn replace_or_append_string_list_key(content: &str, key: &str, values: &[String]) -> String {
    let rendered = format!("{key} = {}", render_string_list(values));
    let mut replaced = false;
    let mut lines = Vec::new();
    for line in content.lines() {
        let trimmed = line.trim_start();
        if !replaced
            && trimmed
                .strip_prefix(key)
                .is_some_and(|rest| rest.trim_start().starts_with('='))
        {
            lines.push(rendered.clone());
            replaced = true;
        } else {
            lines.push(line.to_string());
        }
    }
    if !replaced {
        if !lines.is_empty() {
            lines.push(String::new());
        }
        lines.push(rendered);
    }
    let mut updated = lines.join("\n");
    updated.push('\n');
    updated
}

fn render_default_config(config: &AppConfig) -> String {
    format!(
        r#"# DeepSeekCode project configuration
model.base_url = "{base_url}"
model.model = "{model}"
model.api_key_env = "{api_key_env}"
model.reasoning_effort = "{reasoning_effort}"

# Optional OpenAI-compatible vision model for the image_analyze tool.
vision.base_url = "{vision_base_url}"
vision.model = "{vision_model}"
vision.api_key_env = "{vision_api_key_env}"

approval.require_write_confirmation = {require_write_confirmation}
approval.require_shell_confirmation = {require_shell_confirmation}
approval.require_mcp_confirmation = {require_mcp_confirmation}
approval.mcp_call_allowlist = {mcp_call_allowlist}

workspace.config_dir = "{config_dir}"
workspace.session_dir = "{session_dir}"
workspace.user_skills_dir = "{user_skills_dir}"
workspace.user_commands_dir = "{user_commands_dir}"
workspace.user_instructions_file = "{user_instructions_file}"

# Hooks are disabled by default. Enable only for hook scripts you trust.
hooks.enabled = {hooks_enabled}
hooks.project_dir = "{hooks_project_dir}"
hooks.user_dir = "{hooks_user_dir}"
hooks.timeout_ms = {hooks_timeout_ms}

# MCP server discovery supports config inspection plus stdio/http/sse tools/list/call.
# Keep dynamic remote tool exposure off unless you trust the configured MCP servers.
# Use `deepseek mcp list|doctor|tools|call` to inspect or invoke MCP definitions.
mcp.enabled = {mcp_enabled}
mcp.expose_remote_tools = {mcp_expose_remote_tools}
mcp.project_file = "{mcp_project_file}"
mcp.user_file = "{mcp_user_file}"

# Diagnostics can be run manually with `deepseek diagnostics`.
# Set post_edit to true to append diagnostics after successful apply_patch calls.
diagnostics.post_edit = {diagnostics_post_edit}

# User memory is opt-in. `note` appends to notes_path; `remember` is exposed
# only when memory.enabled is true and appends to memory_path.
memory.enabled = {memory_enabled}
memory.notes_path = "{memory_notes_path}"
memory.memory_path = "{memory_memory_path}"

# Read-only web/search/fetch tools honor this DeepSeek-TUI-style host policy.
# Deny entries win over allow entries. A leading dot matches subdomains only.
network.default = "{network_default}"
network.allow = {network_allow}
network.deny = {network_deny}
network.audit = {network_audit}
network.audit_path = "{network_audit_path}"
"#,
        base_url = config.model.base_url,
        model = config.model.model,
        api_key_env = config.model.api_key_env,
        reasoning_effort = config.model.reasoning_effort,
        vision_base_url = config.vision.base_url,
        vision_model = config.vision.model,
        vision_api_key_env = config.vision.api_key_env,
        require_write_confirmation = config.approval.require_write_confirmation,
        require_shell_confirmation = config.approval.require_shell_confirmation,
        require_mcp_confirmation = config.approval.require_mcp_confirmation,
        mcp_call_allowlist = render_string_list(&config.approval.mcp_call_allowlist),
        config_dir = config.workspace.config_dir,
        session_dir = config.workspace.session_dir,
        user_skills_dir = config.workspace.user_skills_dir,
        user_commands_dir = config.workspace.user_commands_dir,
        user_instructions_file = config.workspace.user_instructions_file,
        hooks_enabled = config.hooks.enabled,
        hooks_project_dir = config.hooks.project_dir,
        hooks_user_dir = config.hooks.user_dir,
        hooks_timeout_ms = config.hooks.timeout_ms,
        mcp_enabled = config.mcp.enabled,
        mcp_expose_remote_tools = config.mcp.expose_remote_tools,
        mcp_project_file = config.mcp.project_file,
        mcp_user_file = config.mcp.user_file,
        diagnostics_post_edit = config.diagnostics.post_edit,
        memory_enabled = config.memory.enabled,
        memory_notes_path = config.memory.notes_path,
        memory_memory_path = config.memory.memory_path,
        network_default = config.network.default,
        network_allow = render_string_list(&config.network.allow),
        network_deny = render_string_list(&config.network.deny),
        network_audit = config.network.audit,
        network_audit_path = config.network.audit_path,
    )
}

fn render_string_list(values: &[String]) -> String {
    if values.is_empty() {
        return "[]".to_string();
    }
    format!(
        "[{}]",
        values
            .iter()
            .map(|value| format!("\"{}\"", value.replace('"', "\\\"")))
            .collect::<Vec<_>>()
            .join(", ")
    )
}

fn render_default_mcp_config() -> &'static str {
    r#"{
  "mcpServers": {
    "example-filesystem": {
      "disabled": true,
      "transport": "stdio",
      "command": "npx",
      "args": ["-y", "@modelcontextprotocol/server-filesystem", "."]
    }
  }
}
"#
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root(name: &str) -> std::path::PathBuf {
        let suffix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "deepseek-config-{name}-{}-{suffix}",
            std::process::id()
        ))
    }

    #[test]
    fn init_config_creates_project_bootstrap_files() {
        let root = temp_root("init");
        let path = init_config_at(&root, false).unwrap();

        assert_eq!(path, root.join(".dscode/config.toml"));
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("model.base_url"));
        assert!(content.contains("vision.model"));
        assert!(content.contains("network.default"));
        assert!(content.contains("network.audit_path"));
        assert!(content.contains("hooks.enabled = false"));
        assert!(root.join(".dscode/sessions").is_dir());
        assert!(root.join(".dscode/commands").is_dir());
        assert!(root.join(".dscode/hooks/pre_tool_use").is_dir());
        assert!(root.join(".dscode/hooks/shell_env").is_dir());
        assert!(root.join(".dscode/mcp.json").is_file());
        let mcp = std::fs::read_to_string(root.join(".dscode/mcp.json")).unwrap();
        assert!(mcp.contains("mcpServers"));

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn init_config_refuses_existing_file_without_force() {
        let root = temp_root("exists");
        let path = init_config_at(&root, false).unwrap();
        std::fs::write(&path, "sentinel").unwrap();

        let error = init_config_at(&root, false).unwrap_err();
        assert!(error.to_string().contains("config already exists"));

        init_config_at(&root, true).unwrap();
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("DeepSeekCode project configuration"));

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn persist_network_allow_adds_normalized_host_to_config() {
        let root = temp_root("network-allow");
        init_config_at(&root, false).unwrap();

        let result =
            persist_network_rule_at(&root, "*.Example.com", NetworkRuleTarget::Allow).unwrap();

        assert!(result.changed);
        assert_eq!(result.host, ".example.com");
        let content = std::fs::read_to_string(root.join(".dscode/config.toml")).unwrap();
        assert!(content.contains(r#"network.allow = [".example.com"]"#));

        let second =
            persist_network_rule_at(&root, ".example.com", NetworkRuleTarget::Allow).unwrap();
        assert!(!second.changed);

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn persist_network_allow_refuses_matching_deny_rule() {
        let root = temp_root("network-deny-wins");
        init_config_at(&root, false).unwrap();
        persist_network_rule_at(&root, ".example.com", NetworkRuleTarget::Deny).unwrap();

        let error = persist_network_rule_at(&root, "api.example.com", NetworkRuleTarget::Allow)
            .unwrap_err();

        assert!(error.to_string().contains("network.deny already matches"));

        let _ = std::fs::remove_dir_all(root);
    }
}
