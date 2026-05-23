use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use crate::cli::app::{SkillsAction, SkillsListArgs, SkillsValidateArgs};
use crate::config::load::load_or_default;
use crate::config::types::AppConfig;
use crate::core::todos::TodoList;
use crate::error::{app_error, AppResult};
use crate::skills::loader::load_skill;
use crate::skills::schema::SkillSpec;
use crate::tools::registry::{default_registry_with_context, ExecutionPolicy};
use crate::util::json::{json_value_to_string, JsonValue};

pub fn run(action: SkillsAction) -> AppResult<()> {
    let config = load_or_default()?;
    match action {
        SkillsAction::List(args) => list_skills(&config, args),
        SkillsAction::Validate(args) => validate_skills(&config, args),
    }
}

#[derive(Debug, Clone)]
struct SkillValidationReport {
    dirs: Vec<SkillDirReport>,
    entries: Vec<SkillEntryReport>,
    overrides: Vec<SkillOverrideReport>,
    warning_count: usize,
    error_count: usize,
}

#[derive(Debug, Clone)]
struct SkillDirReport {
    path: PathBuf,
    exists: bool,
    is_dir: bool,
    count: usize,
    error: Option<String>,
}

#[derive(Debug, Clone)]
struct SkillEntryReport {
    path: PathBuf,
    name: Option<String>,
    description: String,
    allowed_tools: Vec<String>,
    triggers: Vec<String>,
    warnings: Vec<String>,
    error: Option<String>,
}

#[derive(Debug, Clone)]
struct SkillOverrideReport {
    name: String,
    original_path: PathBuf,
    overriding_path: PathBuf,
}

fn list_skills(config: &AppConfig, args: SkillsListArgs) -> AppResult<()> {
    let dirs = resolve_skill_dirs(config, &args.dirs);
    let report = scan_skill_dirs(&dirs)?;
    if args.json {
        println!(
            "{}",
            render_skill_report_json("deepseek.skills_list.v1", &report, false)
        );
    } else {
        print_skill_list_report(&report);
    }
    Ok(())
}

fn validate_skills(config: &AppConfig, args: SkillsValidateArgs) -> AppResult<()> {
    let dirs = resolve_skill_dirs(config, &args.dirs);
    let report = scan_skill_dirs(&dirs)?;
    if args.json {
        println!(
            "{}",
            render_skill_report_json("deepseek.skills_validate.v1", &report, args.strict)
        );
    } else {
        print_skill_validate_report(&report, args.strict);
    }
    if report.error_count > 0 || (args.strict && report.warning_count > 0) {
        return Err(app_error(format!(
            "skills validation failed (errors={}, warnings={}, strict={})",
            report.error_count, report.warning_count, args.strict
        )));
    }
    Ok(())
}

fn resolve_skill_dirs(config: &AppConfig, explicit_dirs: &[String]) -> Vec<PathBuf> {
    if explicit_dirs.is_empty() {
        return vec![
            crate::skills::paths::resolve_repo_skills_dir(),
            crate::skills::tilde::expand_tilde(&config.workspace.user_skills_dir),
        ];
    }
    explicit_dirs
        .iter()
        .map(|dir| crate::skills::tilde::expand_tilde(dir))
        .collect()
}

fn scan_skill_dirs(dirs: &[PathBuf]) -> AppResult<SkillValidationReport> {
    let known_tools = known_tool_names();
    let mut report = SkillValidationReport {
        dirs: Vec::new(),
        entries: Vec::new(),
        overrides: Vec::new(),
        warning_count: 0,
        error_count: 0,
    };
    let mut seen: BTreeMap<String, PathBuf> = BTreeMap::new();

    for dir in dirs {
        let exists = dir.exists();
        let is_dir = dir.is_dir();
        let mut dir_report = SkillDirReport {
            path: dir.clone(),
            exists,
            is_dir,
            count: 0,
            error: None,
        };

        if !exists {
            report.dirs.push(dir_report);
            continue;
        }
        if !is_dir {
            dir_report.error = Some("path exists but is not a directory".to_string());
            report.error_count += 1;
            report.dirs.push(dir_report);
            continue;
        }

        let mut files = skill_files_in_dir(dir)?;
        files.sort();
        dir_report.count = files.len();

        for path in files {
            match load_skill(&path) {
                Ok(skill) => {
                    let warnings = validate_skill_metadata(&skill, &known_tools);
                    report.warning_count += warnings.len();
                    if let Some(original_path) = seen.insert(skill.name.clone(), path.clone()) {
                        report.overrides.push(SkillOverrideReport {
                            name: skill.name.clone(),
                            original_path,
                            overriding_path: path.clone(),
                        });
                    }
                    report.entries.push(SkillEntryReport {
                        path,
                        name: Some(skill.name.clone()),
                        description: skill.description.clone(),
                        allowed_tools: skill.allowed_tools.clone(),
                        triggers: skill.triggers.clone(),
                        warnings,
                        error: None,
                    });
                }
                Err(error) => {
                    report.error_count += 1;
                    report.entries.push(SkillEntryReport {
                        path,
                        name: None,
                        description: String::new(),
                        allowed_tools: Vec::new(),
                        triggers: Vec::new(),
                        warnings: Vec::new(),
                        error: Some(error.to_string()),
                    });
                }
            }
        }

        report.dirs.push(dir_report);
    }

    Ok(report)
}

fn skill_files_in_dir(dir: &Path) -> AppResult<Vec<PathBuf>> {
    let mut files = Vec::new();
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        if path.extension().and_then(|ext| ext.to_str()) == Some("toml") {
            files.push(path);
        }
    }
    Ok(files)
}

fn validate_skill_metadata(skill: &SkillSpec, known_tools: &BTreeSet<String>) -> Vec<String> {
    let mut warnings = Vec::new();
    if skill.name.trim().is_empty() {
        warnings.push("name is empty".to_string());
    }
    if skill.description.trim().is_empty() {
        warnings.push("description is empty".to_string());
    }
    if skill.system_append.trim().is_empty() {
        warnings.push("system_append is empty".to_string());
    }
    if skill.suggested_steps.is_empty() {
        warnings.push("suggested_steps is empty".to_string());
    }
    if skill.allowed_tools.is_empty() {
        warnings.push("allowed_tools is empty; the skill can use all tools".to_string());
    }
    for tool in &skill.allowed_tools {
        if !is_known_skill_tool(tool, known_tools) {
            warnings.push(format!(
                "allowed_tools entry `{tool}` does not match a known tool"
            ));
        }
    }
    warnings
}

fn known_tool_names() -> BTreeSet<String> {
    let registry = default_registry_with_context(
        AppConfig::default(),
        0,
        Rc::new(RefCell::new(TodoList::default())),
    );
    let policy = ExecutionPolicy::new(&AppConfig::default().approval, None);
    let mut names = registry
        .names_for_policy(&policy)
        .into_iter()
        .map(str::to_string)
        .collect::<BTreeSet<_>>();
    for optional in [
        "remember",
        "mcp_list_tools",
        "mcp_call",
        "mcp_list_prompts",
        "mcp_get_prompt",
        "mcp_list_resources",
        "mcp_read_resource",
        "mcp_list_resource_templates",
    ] {
        names.insert(optional.to_string());
    }
    names
}

fn is_known_skill_tool(tool: &str, known_tools: &BTreeSet<String>) -> bool {
    known_tools.contains(tool) || tool.starts_with("mcp__")
}

fn print_skill_list_report(report: &SkillValidationReport) {
    println!(
        "Skills: {} file(s), {} error(s), {} warning(s)",
        report.entries.len(),
        report.error_count,
        report.warning_count
    );
    print_skill_dirs(report);
    if report.entries.is_empty() {
        println!("No skill files found.");
        return;
    }

    println!();
    println!("Loaded skills:");
    for entry in &report.entries {
        match (&entry.name, &entry.error) {
            (Some(name), None) => {
                let triggers = if entry.triggers.is_empty() {
                    "-".to_string()
                } else {
                    entry.triggers.join(", ")
                };
                println!(
                    "  {name:<22} {} (tools={}, triggers={})",
                    entry.description,
                    entry.allowed_tools.len(),
                    triggers
                );
            }
            (_, Some(error)) => {
                println!("  error: {}: {error}", entry.path.display());
            }
            _ => {}
        }
    }
    print_skill_overrides(report);
}

fn print_skill_validate_report(report: &SkillValidationReport, strict: bool) {
    let ok = report.error_count == 0 && (!strict || report.warning_count == 0);
    println!(
        "Skills validation: {} (files={}, errors={}, warnings={}, strict={})",
        if ok { "ok" } else { "failed" },
        report.entries.len(),
        report.error_count,
        report.warning_count,
        strict
    );
    print_skill_dirs(report);
    for entry in &report.entries {
        if entry.error.is_none() && entry.warnings.is_empty() {
            continue;
        }
        println!();
        println!("{}:", entry.path.display());
        if let Some(error) = &entry.error {
            println!("  error: {error}");
        }
        for warning in &entry.warnings {
            println!("  warning: {warning}");
        }
    }
    print_skill_overrides(report);
}

fn print_skill_dirs(report: &SkillValidationReport) {
    println!("Skill dirs:");
    for dir in &report.dirs {
        if let Some(error) = &dir.error {
            println!("  {}: error ({error})", dir.path.display());
        } else if dir.exists && dir.is_dir {
            println!("  {}: {} skill file(s)", dir.path.display(), dir.count);
        } else {
            println!("  {}: not found (skip)", dir.path.display());
        }
    }
}

fn print_skill_overrides(report: &SkillValidationReport) {
    if report.overrides.is_empty() {
        return;
    }
    println!();
    println!("Overrides:");
    for item in &report.overrides {
        println!(
            "  {}: {} overrides {}",
            item.name,
            item.overriding_path.display(),
            item.original_path.display()
        );
    }
}

fn render_skill_report_json(kind: &str, report: &SkillValidationReport, strict: bool) -> String {
    let ok = report.error_count == 0 && (!strict || report.warning_count == 0);
    let root = object([
        ("kind", JsonValue::String(kind.to_string())),
        ("strict", JsonValue::Bool(strict)),
        ("ok", JsonValue::Bool(ok)),
        (
            "total_files",
            JsonValue::Number(report.entries.len().to_string()),
        ),
        (
            "valid_files",
            JsonValue::Number(
                report
                    .entries
                    .iter()
                    .filter(|entry| entry.error.is_none())
                    .count()
                    .to_string(),
            ),
        ),
        (
            "error_count",
            JsonValue::Number(report.error_count.to_string()),
        ),
        (
            "warning_count",
            JsonValue::Number(report.warning_count.to_string()),
        ),
        (
            "dirs",
            JsonValue::Array(report.dirs.iter().map(render_dir_json).collect()),
        ),
        (
            "entries",
            JsonValue::Array(report.entries.iter().map(render_entry_json).collect()),
        ),
        (
            "overrides",
            JsonValue::Array(report.overrides.iter().map(render_override_json).collect()),
        ),
    ]);
    json_value_to_string(&JsonValue::Object(root))
}

fn render_dir_json(dir: &SkillDirReport) -> JsonValue {
    JsonValue::Object(object([
        ("path", JsonValue::String(dir.path.display().to_string())),
        ("exists", JsonValue::Bool(dir.exists)),
        ("is_dir", JsonValue::Bool(dir.is_dir)),
        ("count", JsonValue::Number(dir.count.to_string())),
        (
            "error",
            dir.error
                .clone()
                .map(JsonValue::String)
                .unwrap_or(JsonValue::Null),
        ),
    ]))
}

fn render_entry_json(entry: &SkillEntryReport) -> JsonValue {
    JsonValue::Object(object([
        ("path", JsonValue::String(entry.path.display().to_string())),
        (
            "name",
            entry
                .name
                .clone()
                .map(JsonValue::String)
                .unwrap_or(JsonValue::Null),
        ),
        ("description", JsonValue::String(entry.description.clone())),
        (
            "allowed_tools",
            string_array(entry.allowed_tools.iter().map(String::as_str)),
        ),
        (
            "triggers",
            string_array(entry.triggers.iter().map(String::as_str)),
        ),
        (
            "warnings",
            string_array(entry.warnings.iter().map(String::as_str)),
        ),
        (
            "error",
            entry
                .error
                .clone()
                .map(JsonValue::String)
                .unwrap_or(JsonValue::Null),
        ),
    ]))
}

fn render_override_json(item: &SkillOverrideReport) -> JsonValue {
    JsonValue::Object(object([
        ("name", JsonValue::String(item.name.clone())),
        (
            "original_path",
            JsonValue::String(item.original_path.display().to_string()),
        ),
        (
            "overriding_path",
            JsonValue::String(item.overriding_path.display().to_string()),
        ),
    ]))
}

fn object(
    items: impl IntoIterator<Item = (&'static str, JsonValue)>,
) -> BTreeMap<String, JsonValue> {
    items
        .into_iter()
        .map(|(key, value)| (key.to_string(), value))
        .collect()
}

fn string_array<'a>(items: impl IntoIterator<Item = &'a str>) -> JsonValue {
    JsonValue::Array(
        items
            .into_iter()
            .map(|item| JsonValue::String(item.to_string()))
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_root(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "deepseek_skills_command_test_{name}_{}_{nanos}",
            std::process::id()
        ))
    }

    fn write_skill(dir: &Path, file: &str, body: &str) {
        fs::create_dir_all(dir).unwrap();
        fs::write(dir.join(file), body).unwrap();
    }

    #[test]
    fn scan_reports_warnings_for_incomplete_skill_metadata() {
        let dir = temp_root("warnings");
        write_skill(
            &dir,
            "broken.toml",
            r#"name = "broken"
allowed_tools = ["definitely_not_a_tool"]
"#,
        );

        let report = scan_skill_dirs(&[dir.clone()]).unwrap();
        assert_eq!(report.error_count, 0);
        assert!(report.warning_count >= 4);
        let entry = report.entries.first().unwrap();
        assert!(entry
            .warnings
            .iter()
            .any(|warning| warning.contains("description is empty")));
        assert!(entry
            .warnings
            .iter()
            .any(|warning| warning.contains("definitely_not_a_tool")));

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn scan_reports_override_without_counting_it_as_warning() {
        let repo = temp_root("override_repo");
        let user = temp_root("override_user");
        let body = r#"name = "shared"
description = "Shared skill"
allowed_tools = ["read_file"]
system_append = "Read carefully"
suggested_steps = ["Read"]
"#;
        write_skill(&repo, "shared.toml", body);
        write_skill(&user, "shared.toml", body);

        let report = scan_skill_dirs(&[repo.clone(), user.clone()]).unwrap();
        assert_eq!(report.error_count, 0);
        assert_eq!(report.warning_count, 0);
        assert_eq!(report.overrides.len(), 1);
        assert_eq!(report.overrides[0].name, "shared");

        let _ = fs::remove_dir_all(repo);
        let _ = fs::remove_dir_all(user);
    }

    #[test]
    fn validation_json_includes_counts_and_entries() {
        let dir = temp_root("json");
        write_skill(
            &dir,
            "ok.toml",
            r#"name = "ok"
description = "OK skill"
allowed_tools = ["read_file"]
system_append = "Read carefully"
suggested_steps = ["Read"]
"#,
        );

        let report = scan_skill_dirs(&[dir.clone()]).unwrap();
        let rendered = render_skill_report_json("deepseek.skills_validate.v1", &report, true);
        assert!(rendered.contains("\"kind\":\"deepseek.skills_validate.v1\""));
        assert!(rendered.contains("\"ok\":true"));
        assert!(rendered.contains("\"total_files\":1"));
        assert!(rendered.contains("\"name\":\"ok\""));

        let _ = fs::remove_dir_all(dir);
    }
}
