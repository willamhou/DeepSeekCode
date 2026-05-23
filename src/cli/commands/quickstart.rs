use std::collections::BTreeMap;
use std::env;
use std::fmt::Write as _;
use std::io::{self, IsTerminal};

use crate::cli::app::QuickstartArgs;
use crate::config::load::load_or_default;
use crate::config::types::AppConfig;
use crate::error::AppResult;
use crate::util::json::{json_value_to_string, JsonValue};

pub fn run(args: QuickstartArgs) -> AppResult<()> {
    let config = load_or_default()?;
    let report = build_quickstart_report(
        &config,
        io::stdin().is_terminal() && io::stdout().is_terminal(),
    );

    if args.json {
        println!("{}", render_json_report(&report));
    } else {
        print!("{}", render_text_report(&report));
    }

    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct QuickstartReport {
    version: String,
    cwd: String,
    config_path: String,
    config_present: bool,
    api_key_env: String,
    api_key_present: bool,
    terminal_tty: bool,
    next_commands: Vec<String>,
    first_tasks: Vec<String>,
}

impl QuickstartReport {
    fn ready_for_live_model(&self) -> bool {
        self.config_present && self.api_key_present
    }
}

fn build_quickstart_report(config: &AppConfig, terminal_tty: bool) -> QuickstartReport {
    let cwd = env::current_dir()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|_| ".".to_string());
    let config_path = config.workspace.config_path();
    let config_present = config_path.exists();
    let api_key_present = model_api_key_present(config);

    build_quickstart_report_from_state(
        config,
        cwd,
        config_path.display().to_string(),
        config_present,
        api_key_present,
        terminal_tty,
    )
}

fn build_quickstart_report_from_state(
    config: &AppConfig,
    cwd: String,
    config_path: String,
    config_present: bool,
    api_key_present: bool,
    terminal_tty: bool,
) -> QuickstartReport {
    QuickstartReport {
        version: env!("CARGO_PKG_VERSION").to_string(),
        cwd,
        config_path,
        config_present,
        api_key_env: config.model.api_key_env.clone(),
        api_key_present,
        terminal_tty,
        next_commands: quickstart_next_commands(
            &config.model.api_key_env,
            config_present,
            api_key_present,
        ),
        first_tasks: first_tasks(),
    }
}

fn quickstart_next_commands(
    api_key_env: &str,
    config_present: bool,
    api_key_present: bool,
) -> Vec<String> {
    let mut commands = Vec::new();
    if !config_present {
        commands.push("deepseek config init".to_string());
    }
    if !api_key_present {
        commands.push(format!(
            "printf '%s\\n' '<api-key>' | deepseek config auth {api_key_env} --stdin"
        ));
    }
    commands.push("deepseek doctor --json".to_string());
    if config_present && api_key_present {
        commands.push("deepseek smoke".to_string());
        commands.push("deepseek".to_string());
    } else {
        commands.push("deepseek quickstart".to_string());
    }
    commands
}

fn first_tasks() -> Vec<String> {
    vec![
        "deepseek run \"explain this repository structure\"".to_string(),
        "deepseek run \"review the current diff and call out risks\"".to_string(),
        "deepseek run \"find one small improvement, implement it, and run the relevant check\""
            .to_string(),
    ]
}

fn model_api_key_present(config: &AppConfig) -> bool {
    env::var(&config.model.api_key_env).is_ok_and(|value| !value.trim().is_empty())
}

fn render_text_report(report: &QuickstartReport) -> String {
    let mut out = String::new();
    writeln!(&mut out, "DeepSeekCode quickstart").expect("write to string");
    writeln!(&mut out).expect("write to string");
    writeln!(&mut out, "Status").expect("write to string");
    writeln!(
        &mut out,
        "- workspace config: {} ({})",
        status_label(report.config_present),
        report.config_path
    )
    .expect("write to string");
    writeln!(
        &mut out,
        "- API key: {} ({})",
        status_label(report.api_key_present),
        report.api_key_env
    )
    .expect("write to string");
    writeln!(
        &mut out,
        "- terminal: {}",
        if report.terminal_tty {
            "interactive"
        } else {
            "non-interactive"
        }
    )
    .expect("write to string");
    writeln!(
        &mut out,
        "- live model ready: {}",
        yes_no(report.ready_for_live_model())
    )
    .expect("write to string");
    writeln!(&mut out, "- version: {}", report.version).expect("write to string");
    writeln!(&mut out, "- cwd: {}", report.cwd).expect("write to string");

    writeln!(&mut out).expect("write to string");
    writeln!(&mut out, "Next commands").expect("write to string");
    for (index, command) in report.next_commands.iter().enumerate() {
        writeln!(&mut out, "{}. {}", index + 1, command).expect("write to string");
    }

    writeln!(&mut out).expect("write to string");
    writeln!(&mut out, "Starter tasks").expect("write to string");
    for task in &report.first_tasks {
        writeln!(&mut out, "- {task}").expect("write to string");
    }

    if !report.api_key_present || !report.terminal_tty {
        writeln!(&mut out).expect("write to string");
        writeln!(&mut out, "Notes").expect("write to string");
    }
    if !report.api_key_present {
        writeln!(
            &mut out,
            "- The auth command reads the key from stdin and does not print the value."
        )
        .expect("write to string");
    }
    if !report.terminal_tty {
        writeln!(
            &mut out,
            "- Non-interactive shells should prefer `deepseek run \"<task>\"`."
        )
        .expect("write to string");
    }

    out
}

fn render_json_report(report: &QuickstartReport) -> String {
    json_value_to_string(&JsonValue::Object(object([
        (
            "kind",
            JsonValue::String("deepseek.quickstart.v1".to_string()),
        ),
        ("version", JsonValue::String(report.version.clone())),
        ("cwd", JsonValue::String(report.cwd.clone())),
        ("config_path", JsonValue::String(report.config_path.clone())),
        ("config_present", JsonValue::Bool(report.config_present)),
        ("api_key_env", JsonValue::String(report.api_key_env.clone())),
        ("api_key_present", JsonValue::Bool(report.api_key_present)),
        ("terminal_tty", JsonValue::Bool(report.terminal_tty)),
        (
            "ready_for_live_model",
            JsonValue::Bool(report.ready_for_live_model()),
        ),
        (
            "next_commands",
            json_string_array(report.next_commands.clone()),
        ),
        ("first_tasks", json_string_array(report.first_tasks.clone())),
    ])))
}

fn status_label(value: bool) -> &'static str {
    if value {
        "ok"
    } else {
        "missing"
    }
}

fn yes_no(value: bool) -> &'static str {
    if value {
        "yes"
    } else {
        "no"
    }
}

fn object<const N: usize>(items: [(&str, JsonValue); N]) -> BTreeMap<String, JsonValue> {
    let mut map = BTreeMap::new();
    for (key, value) in items {
        map.insert(key.to_string(), value);
    }
    map
}

fn json_string_array(values: Vec<String>) -> JsonValue {
    JsonValue::Array(values.into_iter().map(JsonValue::String).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::json::{json_as_array, json_as_string, parse_root_object};

    #[test]
    fn missing_state_lists_setup_commands() {
        let config = test_config("TEST_DEEPSEEK_API_KEY");
        let report = build_quickstart_report_from_state(
            &config,
            "/repo".to_string(),
            ".dscode/config.toml".to_string(),
            false,
            false,
            true,
        );

        assert_eq!(report.next_commands[0], "deepseek config init");
        assert!(report
            .next_commands
            .iter()
            .any(|command| command.contains("config auth TEST_DEEPSEEK_API_KEY --stdin")));
        assert!(!report.ready_for_live_model());

        let text = render_text_report(&report);
        assert!(text.contains("workspace config: missing"));
        assert!(text.contains("API key: missing (TEST_DEEPSEEK_API_KEY)"));
    }

    #[test]
    fn ready_state_lists_live_commands() {
        let config = test_config("READY_DEEPSEEK_API_KEY");
        let report = build_quickstart_report_from_state(
            &config,
            "/repo".to_string(),
            ".dscode/config.toml".to_string(),
            true,
            true,
            true,
        );

        assert!(report.ready_for_live_model());
        assert!(!report
            .next_commands
            .iter()
            .any(|command| command == "deepseek config init"));
        assert!(report
            .next_commands
            .iter()
            .any(|command| command == "deepseek smoke"));
        assert!(report
            .next_commands
            .iter()
            .any(|command| command == "deepseek"));
    }

    #[test]
    fn json_report_is_stable_and_secret_free() {
        let config = test_config("JSON_DEEPSEEK_API_KEY");
        let report = build_quickstart_report_from_state(
            &config,
            "/repo".to_string(),
            ".dscode/config.toml".to_string(),
            true,
            true,
            false,
        );
        let json = render_json_report(&report);
        let root = parse_root_object(&json).expect("json root should parse");

        assert_eq!(
            json_as_string(root.get("kind").expect("kind")),
            Some("deepseek.quickstart.v1")
        );
        assert!(matches!(
            root.get("api_key_present"),
            Some(JsonValue::Bool(true))
        ));
        assert_eq!(
            json_as_string(root.get("api_key_env").expect("api key env")),
            Some("JSON_DEEPSEEK_API_KEY")
        );
        assert!(matches!(
            root.get("terminal_tty"),
            Some(JsonValue::Bool(false))
        ));
        assert!(
            json_as_array(root.get("next_commands").expect("next commands"))
                .expect("next commands array")
                .iter()
                .any(|value| json_as_string(value) == Some("deepseek smoke"))
        );
        assert!(!json.contains("sk-should-not-appear"));
    }

    fn test_config(api_key_env: &str) -> AppConfig {
        let mut config = AppConfig::default();
        config.model.api_key_env = api_key_env.to_string();
        config
    }
}
