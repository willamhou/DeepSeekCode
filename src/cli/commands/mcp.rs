use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::cli::app::McpAction;
use crate::config::load::load_or_default;
use crate::config::types::AppConfig;
use crate::error::{app_error, AppResult};
use crate::util::json::{
    json_as_array, json_as_object, json_as_string, parse_root_object, JsonValue,
};

pub fn run(action: McpAction) -> AppResult<()> {
    let config = load_or_default()?;
    match action {
        McpAction::List => list_servers(&config),
        McpAction::Doctor => doctor(&config),
        McpAction::Init { force } => {
            let path = init_mcp_config_at(&std::env::current_dir()?, &config, force)?;
            println!("initialized MCP config: {}", path.display());
            Ok(())
        }
    }
}

fn list_servers(config: &AppConfig) -> AppResult<()> {
    if !config.mcp.enabled {
        println!("MCP is disabled by config: mcp.enabled = false");
        return Ok(());
    }

    let inventory = load_inventory(config)?;
    print_sources(&inventory);

    if inventory.servers.is_empty() {
        println!("No MCP servers configured. Run `deepseek mcp init` to create .dscode/mcp.json.");
        return Ok(());
    }

    println!("MCP servers:");
    for server in &inventory.servers {
        let status = if server.enabled {
            "enabled"
        } else {
            "disabled"
        };
        let detail = match server.transport.as_str() {
            "stdio" => server
                .command
                .as_deref()
                .map(|command| {
                    if server.args.is_empty() {
                        command.to_string()
                    } else {
                        format!("{command} {}", server.args.join(" "))
                    }
                })
                .unwrap_or_else(|| "(missing command)".to_string()),
            _ => server.url.as_deref().unwrap_or("(missing url)").to_string(),
        };
        let env = if server.env_keys.is_empty() {
            "-".to_string()
        } else {
            server.env_keys.join(",")
        };
        println!(
            "- {} [{} {}] {} (source={}, env={})",
            server.name, status, server.transport, detail, server.source, env
        );
    }

    Ok(())
}

fn doctor(config: &AppConfig) -> AppResult<()> {
    if !config.mcp.enabled {
        println!("MCP is disabled by config: mcp.enabled = false");
        return Ok(());
    }

    let inventory = load_inventory(config)?;
    print_sources(&inventory);
    let enabled = inventory
        .servers
        .iter()
        .filter(|server| server.enabled)
        .count();
    println!(
        "mcp doctor: ok ({} server(s), {} enabled)",
        inventory.servers.len(),
        enabled
    );
    Ok(())
}

fn print_sources(inventory: &McpInventory) {
    println!("MCP config sources:");
    for source in &inventory.sources {
        println!("- {}: {}", source.scope, source.path.display());
    }
    if inventory.sources.is_empty() {
        println!("- none found");
    }
}

pub(crate) fn init_mcp_config_at(
    root: &Path,
    config: &AppConfig,
    force: bool,
) -> AppResult<PathBuf> {
    let path = root.join(config.mcp.project_file_path());
    if path.exists() && !force {
        return Err(app_error(format!(
            "MCP config already exists: {} (use --force to overwrite)",
            path.display()
        )));
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, default_mcp_config())?;
    Ok(path)
}

fn default_mcp_config() -> &'static str {
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

fn load_inventory(config: &AppConfig) -> AppResult<McpInventory> {
    let mut inventory = McpInventory::default();
    let mut merged = BTreeMap::<String, McpServer>::new();

    for (scope, path) in [
        ("user", config.mcp.user_file_path()),
        ("project", config.mcp.project_file_path()),
    ] {
        if !path.exists() {
            continue;
        }
        inventory.sources.push(McpSource {
            scope: scope.to_string(),
            path: path.clone(),
        });
        for server in read_mcp_config_file(scope, &path)? {
            merged.insert(server.name.clone(), server);
        }
    }

    inventory.servers = merged.into_values().collect();
    Ok(inventory)
}

fn read_mcp_config_file(scope: &str, path: &Path) -> AppResult<Vec<McpServer>> {
    let content = std::fs::read_to_string(path)?;
    let root = parse_root_object(&content).map_err(|error| {
        app_error(format!(
            "failed to parse MCP config {}: {error}",
            path.display()
        ))
    })?;
    parse_mcp_servers(scope, path, &root)
}

fn parse_mcp_servers(
    scope: &str,
    path: &Path,
    root: &BTreeMap<String, JsonValue>,
) -> AppResult<Vec<McpServer>> {
    let Some(servers_value) = root.get("mcpServers").or_else(|| root.get("servers")) else {
        return Err(app_error(format!(
            "MCP config {} must contain a `mcpServers` object",
            path.display()
        )));
    };
    let Some(servers_object) = json_as_object(servers_value) else {
        return Err(app_error(format!(
            "MCP config {} `mcpServers` must be an object",
            path.display()
        )));
    };

    let mut servers = Vec::new();
    for (name, value) in servers_object {
        let Some(object) = json_as_object(value) else {
            return Err(app_error(format!(
                "MCP server `{name}` in {} must be an object",
                path.display()
            )));
        };
        servers.push(parse_mcp_server(scope, path, name, object)?);
    }
    Ok(servers)
}

fn parse_mcp_server(
    scope: &str,
    path: &Path,
    name: &str,
    object: &BTreeMap<String, JsonValue>,
) -> AppResult<McpServer> {
    let disabled = object
        .get("disabled")
        .and_then(json_as_bool)
        .unwrap_or(false);
    let enabled = object.get("enabled").and_then(json_as_bool).unwrap_or(true) && !disabled;
    let transport = normalize_transport(
        object
            .get("transport")
            .or_else(|| object.get("type"))
            .and_then(json_as_string)
            .unwrap_or_else(|| {
                if object.get("url").is_some() {
                    "http"
                } else {
                    "stdio"
                }
            }),
    )
    .map_err(|error| {
        app_error(format!(
            "MCP server `{name}` in {} has invalid transport: {error}",
            path.display()
        ))
    })?;

    let command = optional_string(object, "command")?;
    let url = optional_string(object, "url")?;
    let args = optional_string_array(object, "args")?;
    let env_keys = optional_object_keys(object, "env")?;
    let header_keys = optional_object_keys(object, "headers")?;

    if enabled && transport == "stdio" && command.as_deref().unwrap_or("").trim().is_empty() {
        return Err(app_error(format!(
            "enabled stdio MCP server `{name}` in {} must define `command`",
            path.display()
        )));
    }
    if enabled && transport != "stdio" && url.as_deref().unwrap_or("").trim().is_empty() {
        return Err(app_error(format!(
            "enabled {transport} MCP server `{name}` in {} must define `url`",
            path.display()
        )));
    }

    Ok(McpServer {
        name: name.to_string(),
        source: scope.to_string(),
        transport,
        enabled,
        command,
        args,
        url,
        env_keys,
        header_keys,
    })
}

fn normalize_transport(raw: &str) -> AppResult<String> {
    match raw {
        "stdio" => Ok("stdio".to_string()),
        "http" | "streamable-http" => Ok("http".to_string()),
        "sse" => Ok("sse".to_string()),
        other => Err(app_error(format!(
            "`{other}` (expected stdio|http|streamable-http|sse)"
        ))),
    }
}

fn optional_string(object: &BTreeMap<String, JsonValue>, key: &str) -> AppResult<Option<String>> {
    let Some(value) = object.get(key) else {
        return Ok(None);
    };
    let Some(value) = json_as_string(value) else {
        return Err(app_error(format!("MCP field `{key}` must be a string")));
    };
    Ok(Some(value.to_string()))
}

fn optional_string_array(
    object: &BTreeMap<String, JsonValue>,
    key: &str,
) -> AppResult<Vec<String>> {
    let Some(value) = object.get(key) else {
        return Ok(Vec::new());
    };
    let Some(items) = json_as_array(value) else {
        return Err(app_error(format!("MCP field `{key}` must be an array")));
    };
    let mut result = Vec::with_capacity(items.len());
    for item in items {
        let Some(value) = json_as_string(item) else {
            return Err(app_error(format!(
                "MCP field `{key}` entries must be strings"
            )));
        };
        result.push(value.to_string());
    }
    Ok(result)
}

fn optional_object_keys(object: &BTreeMap<String, JsonValue>, key: &str) -> AppResult<Vec<String>> {
    let Some(value) = object.get(key) else {
        return Ok(Vec::new());
    };
    let Some(map) = json_as_object(value) else {
        return Err(app_error(format!("MCP field `{key}` must be an object")));
    };
    for (entry_key, entry_value) in map {
        if !matches!(entry_value, JsonValue::String(_)) {
            return Err(app_error(format!(
                "MCP `{key}.{entry_key}` value must be a string"
            )));
        }
    }
    Ok(map.keys().cloned().collect())
}

fn json_as_bool(value: &JsonValue) -> Option<bool> {
    match value {
        JsonValue::Bool(value) => Some(*value),
        _ => None,
    }
}

#[derive(Debug, Default)]
struct McpInventory {
    sources: Vec<McpSource>,
    servers: Vec<McpServer>,
}

#[derive(Debug)]
struct McpSource {
    scope: String,
    path: PathBuf,
}

#[derive(Debug, Clone)]
struct McpServer {
    name: String,
    source: String,
    transport: String,
    enabled: bool,
    command: Option<String>,
    args: Vec<String>,
    url: Option<String>,
    env_keys: Vec<String>,
    #[allow(dead_code)]
    header_keys: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root(name: &str) -> PathBuf {
        let suffix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "deepseek-mcp-{name}-{}-{suffix}",
            std::process::id()
        ))
    }

    #[test]
    fn parse_mcp_servers_reads_stdio_server() {
        let root = parse_root_object(
            r#"{
              "mcpServers": {
                "local": {
                  "command": "node",
                  "args": ["server.js"],
                  "env": {"TOKEN": "value"}
                }
              }
            }"#,
        )
        .unwrap();
        let servers = parse_mcp_servers("project", Path::new(".dscode/mcp.json"), &root).unwrap();

        assert_eq!(servers.len(), 1);
        assert_eq!(servers[0].name, "local");
        assert_eq!(servers[0].transport, "stdio");
        assert_eq!(servers[0].command.as_deref(), Some("node"));
        assert_eq!(servers[0].args, vec!["server.js"]);
        assert_eq!(servers[0].env_keys, vec!["TOKEN"]);
    }

    #[test]
    fn parse_mcp_servers_accepts_disabled_incomplete_server() {
        let root = parse_root_object(
            r#"{
              "mcpServers": {
                "planned": {
                  "disabled": true,
                  "transport": "stdio"
                }
              }
            }"#,
        )
        .unwrap();
        let servers = parse_mcp_servers("project", Path::new(".dscode/mcp.json"), &root).unwrap();

        assert_eq!(servers.len(), 1);
        assert!(!servers[0].enabled);
        assert_eq!(servers[0].command, None);
    }

    #[test]
    fn parse_mcp_servers_rejects_enabled_stdio_without_command() {
        let root = parse_root_object(
            r#"{
              "mcpServers": {
                "bad": {
                  "transport": "stdio"
                }
              }
            }"#,
        )
        .unwrap();
        let error = parse_mcp_servers("project", Path::new(".dscode/mcp.json"), &root)
            .unwrap_err()
            .to_string();

        assert!(error.contains("must define `command`"));
    }

    #[test]
    fn init_mcp_config_refuses_existing_file_without_force() {
        let root = temp_root("init");
        let config = AppConfig::default();
        let path = init_mcp_config_at(&root, &config, false).unwrap();
        std::fs::write(&path, "sentinel").unwrap();

        let error = init_mcp_config_at(&root, &config, false).unwrap_err();
        assert!(error.to_string().contains("already exists"));

        init_mcp_config_at(&root, &config, true).unwrap();
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("mcpServers"));

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn load_inventory_project_overrides_user_server_with_same_name() {
        let root = temp_root("merge");
        std::fs::create_dir_all(&root).unwrap();
        let user_file = root.join("user-mcp.json");
        let project_file = root.join("project-mcp.json");
        std::fs::write(
            &user_file,
            r#"{"mcpServers":{"shared":{"command":"user-server"}}}"#,
        )
        .unwrap();
        std::fs::write(
            &project_file,
            r#"{"mcpServers":{"shared":{"command":"project-server"}}}"#,
        )
        .unwrap();

        let mut config = AppConfig::default();
        config.mcp.user_file = user_file.display().to_string();
        config.mcp.project_file = project_file.display().to_string();
        let inventory = load_inventory(&config).unwrap();

        assert_eq!(inventory.servers.len(), 1);
        assert_eq!(inventory.servers[0].source, "project");
        assert_eq!(
            inventory.servers[0].command.as_deref(),
            Some("project-server")
        );

        let _ = std::fs::remove_dir_all(root);
    }
}
