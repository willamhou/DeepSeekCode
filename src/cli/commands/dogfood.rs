use std::borrow::Cow;
use std::collections::{BTreeMap, HashSet};
use std::env;
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use crate::cli::app::{
    BenchmarkArgs, DogfoodAction, DogfoodCategoryRequirement, DogfoodExportArgs,
    DogfoodExternalEvidenceArgs, DogfoodExternalFixtureArgs, DogfoodLiveEvidenceArgs,
    DogfoodLivePlanArgs, DogfoodLiveRunArgs, DogfoodOutcome, DogfoodPromoteArgs,
    DogfoodRepairCacheEvidenceArgs, DogfoodReplayArgs, DogfoodReportArgs, DogfoodRunArgs,
};
use crate::cli::commands::benchmark::BenchmarkCaseSummary;
use crate::config::load::load_or_default;
use crate::core::context::TaskContext;
use crate::core::loop_runtime::{AgentLoop, AgentLoopOptions, RunResult};
use crate::core::prompt_layers::{
    prompt_layers_event_payload, PromptLayerRecord, PromptLayerSnapshot,
};
use crate::core::runtime::{json_array, json_object, RuntimeStore};
use crate::error::{app_error, AppError, AppErrorKind, AppResult};
use crate::model::protocol::ObservationStatus;
use crate::model::tool_repair::parse_tool_arguments_with_repair;
use crate::tools::read_file::ReadFileTool;
use crate::tools::types::{Tool, ToolInput};
use crate::util::json::{
    json_as_array, json_as_object, json_as_string, json_as_u64, json_escape, json_value_to_string,
    parse_root_object, parse_value, JsonValue,
};

const DEFAULT_REPORT_LIMIT: usize = 20;
const CATEGORY_TREND_WINDOW: usize = 5;
const DEFAULT_LIVE_TARGET_RUNS: usize = 100;
const DEFAULT_LIVE_TARGET_SUCCESS_RATE: f64 = 90.0;
const DEFAULT_LIVE_PLAN_LIMIT: usize = 25;
const DEFAULT_LIVE_RUN_LIMIT: usize = 4;
const MODEL_TRANSPORT_OFFLINE: &str = "offline";
const MODEL_TRANSPORT_ONLINE: &str = "online";
const MODEL_TRANSPORT_UNKNOWN: &str = "unknown";
const LIVE_PLAN_TARGET_CATEGORIES: &[(&str, usize, f64)] = &[
    ("write_validate", 25, 90.0),
    ("recovery", 25, 90.0),
    ("pr_workflow", 25, 90.0),
    ("mcp", 3, 90.0),
];

pub fn run(action: DogfoodAction) -> AppResult<()> {
    let config = load_or_default()?;
    match action {
        DogfoodAction::Run(args) => run_live_task(&config, args),
        DogfoodAction::ExternalFixture(args) => run_external_fixture_command(&config, args),
        DogfoodAction::ExternalEvidence(args) => external_evidence_command(args),
        DogfoodAction::RepairCacheEvidence(args) => repair_cache_evidence_command(&config, args),
        DogfoodAction::ReplayBenchmark(args) => replay_benchmark_command(&config, args),
        DogfoodAction::LivePlan(args) => live_plan_command(&config, args),
        DogfoodAction::LiveRun(args) => live_run_command(&config, args),
        DogfoodAction::LiveEvidence(args) => live_evidence_command(args),
        DogfoodAction::Report(args) => render_report_command(&config, args),
        DogfoodAction::ExportBenchmark(args) => export_benchmark_command(&config, args),
        DogfoodAction::PromoteBenchmark(args) => promote_benchmark_command(&config, args),
    }
}

fn run_live_task(config: &crate::config::types::AppConfig, args: DogfoodRunArgs) -> AppResult<()> {
    run_live_task_with_policy(config, args, DogfoodRunPolicy::default())
}

#[derive(Debug, Clone, Copy, Default)]
struct DogfoodRunPolicy {
    auto_approve_isolated: bool,
}

fn run_live_task_with_policy(
    config: &crate::config::types::AppConfig,
    args: DogfoodRunArgs,
    policy: DogfoodRunPolicy,
) -> AppResult<()> {
    run_live_task_with_policy_and_post_check(config, args, policy, None)
}

fn run_live_task_with_policy_and_post_check(
    config: &crate::config::types::AppConfig,
    args: DogfoodRunArgs,
    policy: DogfoodRunPolicy,
    post_run_check: Option<Box<dyn FnOnce(&Path) -> AppResult<()>>>,
) -> AppResult<()> {
    let args = resolve_run_args(config, args)?;
    let started = Instant::now();
    let repo_root = std::env::current_dir()?;
    let requested_workdir = resolve_run_workdir(&repo_root, args.workdir.as_deref())?;
    let (run_workdir, cleanup_workdir) =
        prepare_run_workdir(&requested_workdir, args.isolate_workdir)?;
    let workdir = run_workdir.display().to_string();
    let budget = args.budget.unwrap_or(AgentLoopOptions::default().steps);
    let manual_intervention =
        args.manual_intervention || matches!(args.outcome, Some(DogfoodOutcome::Manual));
    let model_transport = model_transport_for_config(config);

    println!("DeepSeekCode dogfood");
    println!("task: {}", args.task);
    println!("budget: {budget}");
    println!("workdir: {workdir}");

    let auto_approve = (args.from_benchmark.is_some() || policy.auto_approve_isolated)
        && args.isolate_workdir
        && run_workdir != repo_root;
    let run_result = run_task_in_workdir(&repo_root, &run_workdir, auto_approve, || {
        AgentLoop::new(config.clone()).run_with(
            TaskContext::new(args.task.clone(), args.skill.clone()),
            AgentLoopOptions {
                steps: budget,
                ..AgentLoopOptions::default()
            },
        )
    });
    let run_result = match (run_result, post_run_check) {
        (Ok(result), Some(check)) => check(&run_workdir).map(|()| result),
        (Ok(result), None) => Ok(result),
        (Err(error), _) => Err(error),
    };
    if let Some(path) = cleanup_workdir.as_ref() {
        let _ = fs::remove_dir_all(path);
    }

    let duration_ms = started.elapsed().as_millis() as u64;
    let timestamp_secs = unix_now_secs()?;
    let ledger_path = config.workspace.dogfood_ledger_path();

    if let Err(error) = &run_result {
        if dogfood_error_is_environment_transport_failure(error.as_ref()) {
            println!(
                "ledger: {} (skipped: environment transport failure)",
                ledger_path.display()
            );
            println!(
                "dogfood record skipped: model transport failed before agent execution; report unchanged"
            );
            return match run_result {
                Ok(_) => unreachable!("checked Err above"),
                Err(error) => Err(error),
            };
        }
    }

    let record = match &run_result {
        Ok(result) => DogfoodRecord::from_result(
            timestamp_secs,
            duration_ms,
            config.model.model.clone(),
            model_transport,
            workdir,
            budget,
            &args,
            manual_intervention,
            result,
        ),
        Err(error) => DogfoodRecord::from_error(
            timestamp_secs,
            duration_ms,
            config.model.model.clone(),
            model_transport,
            workdir,
            budget,
            &args,
            manual_intervention,
            error.as_ref(),
        ),
    };

    append_record(&ledger_path, &record)?;
    let records = load_records(&ledger_path)?;
    let report_path = config.workspace.dogfood_report_path();
    write_report(&ledger_path, &report_path, &records, DEFAULT_REPORT_LIMIT)?;

    println!(
        "ledger: {} (outcome: {}, manual_intervention: {})",
        ledger_path.display(),
        record.outcome.label(),
        if record.manual_intervention {
            "yes"
        } else {
            "no"
        }
    );
    println!("report: {}", report_path.display());
    if args.benchmark_gate {
        println!("post-task benchmark gate: running default benchmark baseline");
        crate::cli::commands::benchmark::run_with_config(config.clone(), BenchmarkArgs::default())?;
    }

    match run_result {
        Ok(_) => Ok(()),
        Err(error) => Err(error),
    }
}

fn run_external_fixture_command(
    config: &crate::config::types::AppConfig,
    args: DogfoodExternalFixtureArgs,
) -> AppResult<()> {
    let repo_root = std::env::current_dir()?;
    let requested_workdir = resolve_run_workdir(&repo_root, Some(&args.workdir))?;
    validate_external_fixture_workdir(&repo_root, &requested_workdir)?;
    validate_external_fixture_task(&args.task)?;
    let validation_command = external_fixture_validation_command(&args.task)?;

    println!("DeepSeekCode dogfood external write fixture");
    println!("workdir: {}", requested_workdir.display());
    println!("isolate_workdir: yes");
    println!("auto_approve: isolated writes/shell/mcp");
    let model_transport = model_transport_for_config(config);
    println!("current_model_transport: {model_transport}");
    if args.dry_run {
        if model_transport != MODEL_TRANSPORT_ONLINE {
            println!(
                "release evidence warning: current model transport is not online; real external write-fixture evidence requires an online model-backed run"
            );
        }
        println!("dry run only; no model call, shell command, or ledger write");
        return Ok(());
    }
    validate_external_fixture_model_transport(model_transport, args.allow_offline)?;

    let ledger_path = config.workspace.dogfood_ledger_path();
    let report_path = config.workspace.dogfood_report_path();
    let before_records = load_records_or_empty(&ledger_path)?;
    let validation_command_for_run = validation_command.clone();
    let run_result = run_live_task_with_policy_and_post_check(
        config,
        DogfoodRunArgs {
            task: args.task.clone(),
            from_benchmark: None,
            benchmark_manifest: None,
            skill: None,
            budget: args.budget,
            workdir: Some(requested_workdir.display().to_string()),
            isolate_workdir: true,
            outcome: None,
            manual_intervention: false,
            benchmark_gate: args.benchmark_gate,
            notes: Some(external_fixture_notes(args.notes.as_deref())),
        },
        DogfoodRunPolicy {
            auto_approve_isolated: true,
        },
        Some(Box::new(move |run_workdir| {
            run_external_fixture_post_validation(run_workdir, &validation_command_for_run)
        })),
    );
    let after_records = load_records_or_empty(&ledger_path)?;
    if let Some(evidence_out) = args.evidence_out.as_deref() {
        write_external_fixture_evidence_summary(
            evidence_out,
            &external_fixture_evidence_summary_json(
                &requested_workdir,
                &ledger_path,
                &report_path,
                &args,
                model_transport,
                &before_records,
                &after_records,
                dogfood_file_fingerprint_json(&ledger_path),
                run_result.as_ref().err().map(|error| error.to_string()),
                &validation_command,
            ),
        )?;
        println!("external_fixture_evidence: {evidence_out}");
    }
    run_result
}

fn dogfood_error_is_environment_transport_failure(
    error: &(dyn std::error::Error + 'static),
) -> bool {
    let message = error.to_string().to_lowercase();
    [
        "could not resolve host",
        "temporary failure in name resolution",
        "network is unreachable",
        "connection timed out",
        "curl: (6)",
        "curl: (7)",
        "curl: (28)",
    ]
    .iter()
    .any(|marker| message.contains(marker))
}

fn model_transport_for_config(config: &crate::config::types::AppConfig) -> &'static str {
    let api_key_env = config.model.api_key_env.trim();
    if api_key_env.is_empty() || api_key_env.to_ascii_uppercase().contains("OFFLINE") {
        return MODEL_TRANSPORT_OFFLINE;
    }
    match env::var(api_key_env) {
        Ok(value) if !value.trim().is_empty() => MODEL_TRANSPORT_ONLINE,
        _ => MODEL_TRANSPORT_OFFLINE,
    }
}

fn resolve_run_args(
    config: &crate::config::types::AppConfig,
    mut args: DogfoodRunArgs,
) -> AppResult<DogfoodRunArgs> {
    let Some(case_name) = args.from_benchmark.as_deref() else {
        return Ok(args);
    };
    let manifest_path = args
        .benchmark_manifest
        .as_ref()
        .map(PathBuf::from)
        .unwrap_or_else(|| config.workspace.benchmark_manifest_path());
    let manifest_dir = manifest_path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    let summaries = crate::cli::commands::benchmark::load_manifest_case_summaries(&manifest_path)?;
    let case = summaries
        .into_iter()
        .find(|case| case.name == case_name)
        .ok_or_else(|| {
            app_error(format!(
                "benchmark case `{case_name}` was not found in {}",
                manifest_path.display()
            ))
        })?;
    args.task = case.task;
    if args.skill.is_none() {
        args.skill = case.skill;
    }
    if args.budget.is_none() {
        args.budget = Some(case.budget);
    }
    if args.workdir.is_none() {
        args.workdir = case.workdir.map(|workdir| {
            let candidate = PathBuf::from(&workdir);
            if candidate.is_absolute() {
                workdir
            } else {
                manifest_dir.join(candidate).display().to_string()
            }
        });
    }
    if !args.isolate_workdir {
        args.isolate_workdir = case.isolate_workdir;
    }
    if args.notes.is_none() {
        args.notes = case.notes;
    }
    Ok(args)
}

fn resolve_run_workdir(repo_root: &Path, requested: Option<&str>) -> AppResult<PathBuf> {
    let path = match requested {
        Some(raw) if !raw.trim().is_empty() => {
            let candidate = PathBuf::from(raw);
            if candidate.is_absolute() {
                candidate
            } else {
                repo_root.join(candidate)
            }
        }
        _ => repo_root.to_path_buf(),
    };

    if path.is_dir() {
        Ok(path)
    } else {
        Err(app_error(format!(
            "dogfood workdir does not exist or is not a directory: {}",
            path.display()
        )))
    }
}

fn validate_external_fixture_workdir(repo_root: &Path, workdir: &Path) -> AppResult<()> {
    let repo_root = fs::canonicalize(repo_root).map_err(|error| {
        app_error(format!(
            "failed to canonicalize repository root {}: {error}",
            repo_root.display()
        ))
    })?;
    let workdir = fs::canonicalize(workdir).map_err(|error| {
        app_error(format!(
            "failed to canonicalize external fixture workdir {}: {error}",
            workdir.display()
        ))
    })?;
    if workdir == repo_root || workdir.starts_with(&repo_root) {
        return Err(app_error(format!(
            "external fixture workdir must be outside this repository: {}",
            workdir.display()
        )));
    }
    if !workdir.join(".git").exists() {
        return Err(app_error(format!(
            "external fixture workdir must be a git repository or worktree with .git metadata: {}",
            workdir.display()
        )));
    }
    Ok(())
}

fn validate_external_fixture_task(task: &str) -> AppResult<()> {
    let task_lower = task.to_ascii_lowercase();
    if task_looks_like_write_validate(&task_lower) {
        return Ok(());
    }
    Err(app_error(
        "dogfood external-fixture task must describe an edit and validation command, for example: replace `a - b` with `a + b` in src/lib.rs and validate with cargo test",
    ))
}

fn external_fixture_validation_command(task: &str) -> AppResult<String> {
    let task_lower = task.to_ascii_lowercase();
    let marker = "validate with ";
    let Some(index) = task_lower.rfind(marker) else {
        return Err(app_error(
            "dogfood external-fixture task must include `validate with <command>`",
        ));
    };
    let command = task[index + marker.len()..]
        .trim()
        .trim_end_matches('.')
        .trim();
    if command.is_empty() {
        return Err(app_error(
            "dogfood external-fixture validation command is empty",
        ));
    }
    Ok(command.to_string())
}

fn run_external_fixture_post_validation(workdir: &Path, validation_command: &str) -> AppResult<()> {
    println!("external_fixture_post_validation: {validation_command}");
    let output = validation_shell_command(validation_command)
        .current_dir(workdir)
        .output()
        .map_err(|error| {
            app_error(format!(
                "failed to run external fixture validation `{validation_command}`: {error}"
            ))
        })?;
    if output.status.success() {
        println!("external_fixture_post_validation: pass");
        return Ok(());
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    Err(app_error(format!(
        "external fixture post-validation failed: `{validation_command}` exited with {}; stdout: {}; stderr: {}",
        output.status,
        clip(stdout.trim(), 600),
        clip(stderr.trim(), 600)
    )))
}

#[cfg(windows)]
fn validation_shell_command(validation_command: &str) -> Command {
    let mut command = Command::new("cmd");
    command.args(["/C", validation_command]);
    command
}

#[cfg(not(windows))]
fn validation_shell_command(validation_command: &str) -> Command {
    let mut command = Command::new("sh");
    command.args(["-c", validation_command]);
    command
}

fn validate_external_fixture_model_transport(
    model_transport: &str,
    allow_offline: bool,
) -> AppResult<()> {
    if model_transport == MODEL_TRANSPORT_ONLINE || allow_offline {
        return Ok(());
    }
    Err(app_error(format!(
        "dogfood external-fixture requires online model-backed transport for release evidence; current_model_transport={model_transport}. Use --dry-run to inspect the plan, or pass --allow-offline only for rehearsal runs that will not satisfy release gates"
    )))
}

fn external_fixture_notes(notes: Option<&str>) -> String {
    match notes.map(str::trim).filter(|notes| !notes.is_empty()) {
        Some(notes) => format!("external-write-fixture; {notes}"),
        None => "external-write-fixture".to_string(),
    }
}

fn prepare_run_workdir(
    workdir: &Path,
    isolate_workdir: bool,
) -> AppResult<(PathBuf, Option<PathBuf>)> {
    if !isolate_workdir {
        return Ok((workdir.to_path_buf(), None));
    }
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| app_error(format!("system clock error: {error}")))?
        .as_nanos();

    let temp_root = std::env::temp_dir().join(format!(
        "deepseek-dogfood-{}-{}",
        std::process::id(),
        suffix
    ));
    copy_dir_recursive(workdir, &temp_root)?;
    Ok((temp_root.clone(), Some(temp_root)))
}

fn run_task_in_workdir<T>(
    repo_root: &Path,
    run_workdir: &Path,
    auto_approve: bool,
    f: impl FnOnce() -> AppResult<T>,
) -> AppResult<T> {
    let _cwd_guard = crate::util::cwd::lock_cwd()?;
    let previous_auto_approve_writes = env::var_os("DSCODE_AUTO_APPROVE_WRITES");
    let previous_auto_approve_shell = env::var_os("DSCODE_AUTO_APPROVE_SHELL");
    let previous_auto_approve_mcp = env::var_os("DSCODE_AUTO_APPROVE_MCP");
    let changed_workdir = run_workdir != repo_root;
    if changed_workdir {
        env::set_current_dir(run_workdir)?;
    }
    if auto_approve {
        unsafe {
            env::set_var("DSCODE_AUTO_APPROVE_WRITES", "1");
            env::set_var("DSCODE_AUTO_APPROVE_SHELL", "1");
            env::set_var("DSCODE_AUTO_APPROVE_MCP", "1");
        }
    }
    let result = f();
    let restore_result = if changed_workdir {
        env::set_current_dir(repo_root)
    } else {
        Ok(())
    };
    if auto_approve {
        restore_env_var("DSCODE_AUTO_APPROVE_WRITES", previous_auto_approve_writes);
        restore_env_var("DSCODE_AUTO_APPROVE_SHELL", previous_auto_approve_shell);
        restore_env_var("DSCODE_AUTO_APPROVE_MCP", previous_auto_approve_mcp);
    }
    match (result, restore_result) {
        (Ok(value), Ok(())) => Ok(value),
        (Err(error), Ok(())) => Err(error),
        (Ok(_), Err(error)) => Err(app_error(format!("failed to restore dogfood cwd: {error}"))),
        (Err(error), Err(_restore_error)) => Err(error),
    }
}

fn restore_env_var(name: &str, value: Option<std::ffi::OsString>) {
    match value {
        Some(value) => unsafe { env::set_var(name, value) },
        None => unsafe { env::remove_var(name) },
    }
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> AppResult<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let source = entry.path();
        let target = dst.join(entry.file_name());
        let metadata = entry.metadata()?;
        if metadata.is_dir() {
            copy_dir_recursive(&source, &target)?;
        } else if metadata.is_file() {
            fs::copy(&source, &target)?;
        }
    }
    Ok(())
}

fn render_report_command(
    config: &crate::config::types::AppConfig,
    args: DogfoodReportArgs,
) -> AppResult<()> {
    let ledger_path = config.workspace.dogfood_ledger_path();
    let report_path = args
        .out
        .as_ref()
        .map(PathBuf::from)
        .unwrap_or_else(|| config.workspace.dogfood_report_path());
    let limit = args.limit.unwrap_or(DEFAULT_REPORT_LIMIT);
    let records = load_records(&ledger_path)?;
    write_report(&ledger_path, &report_path, &records, limit)?;
    enforce_report_requirements(&records, &args)?;
    println!("DeepSeekCode dogfood report");
    println!("ledger: {}", ledger_path.display());
    println!("report: {}", report_path.display());
    if report_has_requirements(&args) {
        println!("evidence gates: pass");
    }
    Ok(())
}

fn report_has_requirements(args: &DogfoodReportArgs) -> bool {
    args.require_min_runs.is_some()
        || args.require_success_rate.is_some()
        || args.require_live_runs.is_some()
        || args.require_live_success_rate.is_some()
        || args.require_external_write_fixtures.is_some()
        || args.require_recent_clean.is_some()
        || !args.require_categories.is_empty()
        || !args.require_live_categories.is_empty()
}

fn enforce_report_requirements(
    records: &[DogfoodRecord],
    args: &DogfoodReportArgs,
) -> AppResult<()> {
    let failures = report_requirement_failures(records, args);
    if failures.is_empty() {
        return Ok(());
    }
    Err(app_error(format!(
        "dogfood evidence gates failed:\n- {}",
        failures.join("\n- ")
    )))
}

fn report_requirement_failures(records: &[DogfoodRecord], args: &DogfoodReportArgs) -> Vec<String> {
    let mut failures = Vec::new();
    if let Some(min_runs) = args.require_min_runs {
        if records.len() < min_runs {
            failures.push(format!(
                "runs {} below required minimum {min_runs}",
                records.len()
            ));
        }
    }
    if let Some(min_success_percent) = args.require_success_rate {
        let success = records
            .iter()
            .filter(|record| matches!(record.outcome, DogfoodOutcome::Success))
            .count();
        push_rate_failure(
            &mut failures,
            "overall success rate",
            success,
            records.len(),
            min_success_percent,
        );
    }
    let live_records = records
        .iter()
        .filter(|record| record_is_model_backed(record))
        .collect::<Vec<_>>();
    if let Some(min_runs) = args.require_live_runs {
        if live_records.len() < min_runs {
            failures.push(format!(
                "model-backed runs {} below required minimum {min_runs}",
                live_records.len()
            ));
        }
    }
    if let Some(min_success_percent) = args.require_live_success_rate {
        let live_success = live_records
            .iter()
            .filter(|record| matches!(record.outcome, DogfoodOutcome::Success))
            .count();
        push_rate_failure(
            &mut failures,
            "model-backed success rate",
            live_success,
            live_records.len(),
            min_success_percent,
        );
    }
    if let Some(required) = args.require_external_write_fixtures {
        let successful_external_write_fixtures = records
            .iter()
            .filter(|record| {
                is_external_write_fixture_record(record)
                    && matches!(record.outcome, DogfoodOutcome::Success)
            })
            .count();
        if successful_external_write_fixtures < required {
            failures.push(format!(
                "successful external write fixtures {successful_external_write_fixtures} below required minimum {required}"
            ));
        }
    }
    if let Some(required_clean) = args.require_recent_clean {
        let recent = records
            .iter()
            .rev()
            .take(required_clean)
            .collect::<Vec<_>>();
        if recent.len() < required_clean {
            failures.push(format!(
                "recent clean window has only {} records, required {required_clean}",
                recent.len()
            ));
        }
        let unclean = recent
            .iter()
            .filter(|record| !record_is_clean(record))
            .count();
        if unclean > 0 {
            failures.push(format!(
                "recent clean window contains {unclean} failed, stuck, or manual records"
            ));
        }
    }
    if !args.require_categories.is_empty() {
        let stats = aggregate_category_stats(records);
        for requirement in &args.require_categories {
            push_category_requirement_failure(&mut failures, &stats, requirement, "category");
        }
    }
    if !args.require_live_categories.is_empty() {
        let stats = aggregate_category_stats_for(live_records);
        for requirement in &args.require_live_categories {
            push_category_requirement_failure(
                &mut failures,
                &stats,
                requirement,
                "model-backed category",
            );
        }
    }
    failures
}

fn push_category_requirement_failure(
    failures: &mut Vec<String>,
    stats: &BTreeMap<String, DogfoodCategoryStats>,
    requirement: &DogfoodCategoryRequirement,
    label: &str,
) {
    let Some(category) = stats.get(&requirement.category) else {
        failures.push(format!(
            "{label} `{}` has 0 runs, required {}",
            requirement.category, requirement.min_runs
        ));
        return;
    };
    if category.runs < requirement.min_runs {
        failures.push(format!(
            "{label} `{}` runs {} below required minimum {}",
            requirement.category, category.runs, requirement.min_runs
        ));
    }
    push_rate_failure(
        failures,
        &format!("{label} `{}` success rate", requirement.category),
        category.success,
        category.runs,
        requirement.min_success_percent,
    );
}

fn push_rate_failure(
    failures: &mut Vec<String>,
    label: &str,
    success: usize,
    total: usize,
    min_success_percent: f64,
) {
    let actual = rate_percent(success, total);
    if actual + f64::EPSILON < min_success_percent {
        failures.push(format!(
            "{label} {:.1}% below required {:.1}% ({success}/{total})",
            actual, min_success_percent
        ));
    }
}

fn record_is_clean(record: &DogfoodRecord) -> bool {
    matches!(record.outcome, DogfoodOutcome::Success) && !record.manual_intervention
}

fn record_is_model_backed(record: &DogfoodRecord) -> bool {
    record.model_transport == MODEL_TRANSPORT_ONLINE
}

fn live_plan_command(
    config: &crate::config::types::AppConfig,
    args: DogfoodLivePlanArgs,
) -> AppResult<()> {
    let ledger_path = config.workspace.dogfood_ledger_path();
    let manifest_path = args
        .manifest
        .map(PathBuf::from)
        .unwrap_or_else(|| config.workspace.benchmark_manifest_path());
    let records = load_records_or_empty(&ledger_path)?;
    let summaries = crate::cli::commands::benchmark::load_manifest_case_summaries(&manifest_path)?;
    let targets = live_plan_targets(args.target_categories);
    let plan = build_live_plan(
        &ledger_path,
        &manifest_path,
        &records,
        &summaries,
        model_transport_for_config(config),
        args.target_live_runs.unwrap_or(DEFAULT_LIVE_TARGET_RUNS),
        args.target_live_success_rate
            .unwrap_or(DEFAULT_LIVE_TARGET_SUCCESS_RATE),
        &targets,
        args.limit.unwrap_or(DEFAULT_LIVE_PLAN_LIMIT),
    );
    if args.json {
        println!("{}", render_live_plan_json(&plan));
    } else {
        println!("{}", render_live_plan_text(&plan));
    }
    Ok(())
}

fn live_run_command(
    config: &crate::config::types::AppConfig,
    args: DogfoodLiveRunArgs,
) -> AppResult<()> {
    let _api_key_guard = match args.api_key_file.as_deref() {
        Some(path) => Some(load_live_run_api_key_file(config, path)?),
        None => None,
    };
    let ledger_path = config.workspace.dogfood_ledger_path();
    let manifest_path = args
        .manifest
        .map(PathBuf::from)
        .unwrap_or_else(|| config.workspace.benchmark_manifest_path());
    let records = load_records_or_empty(&ledger_path)?;
    let summaries = crate::cli::commands::benchmark::load_manifest_case_summaries(&manifest_path)?;
    let targets = live_plan_targets(args.target_categories);
    let run_limit = args.limit.unwrap_or(DEFAULT_LIVE_RUN_LIMIT);
    let model_transport = model_transport_for_config(config);
    let plan = build_live_plan(
        &ledger_path,
        &manifest_path,
        &records,
        &summaries,
        model_transport,
        args.target_live_runs.unwrap_or(DEFAULT_LIVE_TARGET_RUNS),
        args.target_live_success_rate
            .unwrap_or(DEFAULT_LIVE_TARGET_SUCCESS_RATE),
        &targets,
        run_limit,
    );
    let selected = select_live_run_cases(&plan, &args.categories, run_limit);

    if args.json {
        if args.execute {
            return Err(app_error(
                "dogfood live-run --json is a dry-run planning output and cannot be combined with --execute",
            ));
        }
        println!(
            "{}",
            render_live_run_plan_json(
                &plan,
                &args.categories,
                run_limit,
                &selected,
                args.api_key_file.as_deref(),
                args.evidence_out.as_deref(),
            )
        );
        return Ok(());
    }

    println!("DeepSeekCode dogfood live run");
    println!("ledger: {}", ledger_path.display());
    println!("manifest: {}", manifest_path.display());
    println!("current_model_transport: {model_transport}");
    if args.api_key_file.is_some() {
        println!(
            "credential_source: api-key-file -> {} (value hidden)",
            config.model.api_key_env
        );
    }
    if let Some(evidence_out) = args.evidence_out.as_deref() {
        println!("evidence_out: {evidence_out}");
    }
    if !args.categories.is_empty() {
        println!("categories: {}", args.categories.join(", "));
    }
    println!(
        "selected: {} (limit: {}, execute: {})",
        selected.len(),
        run_limit,
        if args.execute { "yes" } else { "no" }
    );
    println!("post_run_report_gate: {}", live_report_gate_command(&plan));

    if selected.is_empty() {
        println!("no recommended live dogfood cases matched the requested filters");
        return Ok(());
    }

    for case in &selected {
        println!("planned: {} ({})", case.name, case.category);
    }

    if !args.execute {
        println!("dry run only; add --execute to run model-backed benchmark replays");
        return Ok(());
    }
    if model_transport != MODEL_TRANSPORT_ONLINE {
        return Err(app_error(
            "dogfood live-run --execute requires an online model transport; configure the provider API key first",
        ));
    }

    let before_records = load_records_or_empty(&ledger_path)?;
    let mut latest_records = before_records.clone();
    let mut case_evidence = Vec::new();
    let mut first_run_error = None;

    for case in &selected {
        println!("replay: {} ({})", case.name, case.category);
        let before_case_records = latest_records.len();
        let run_result = run_live_task(
            config,
            DogfoodRunArgs {
                task: String::new(),
                from_benchmark: Some(case.name.clone()),
                benchmark_manifest: Some(manifest_path.display().to_string()),
                skill: None,
                budget: None,
                workdir: None,
                isolate_workdir: false,
                outcome: None,
                manual_intervention: false,
                benchmark_gate: false,
                notes: Some(format!("live-dogfood; category={}", case.category)),
            },
        );
        let after_case_records = load_records_or_empty(&ledger_path)?;
        let appended_records = after_case_records.get(before_case_records..).unwrap_or(&[]);
        let run_error = run_result.as_ref().err().map(|error| error.to_string());
        case_evidence.push(live_run_case_evidence_json(
            case,
            appended_records,
            run_error.as_deref(),
        ));
        latest_records = after_case_records;
        if let Err(error) = run_result {
            first_run_error = Some(error);
            break;
        }
    }

    let mut benchmark_gate_error = None;
    if first_run_error.is_none() && args.benchmark_gate {
        println!("post-live-run benchmark gate: running default benchmark baseline");
        if let Err(error) = crate::cli::commands::benchmark::run_with_config(
            config.clone(),
            BenchmarkArgs::default(),
        ) {
            benchmark_gate_error = Some(error);
        }
    }

    if let Some(evidence_out) = args.evidence_out.as_deref() {
        write_live_run_evidence_summary(
            evidence_out,
            &live_run_evidence_summary_json(
                &plan,
                &args.categories,
                run_limit,
                &selected,
                &case_evidence,
                &before_records,
                &latest_records,
                args.api_key_file.as_deref(),
                args.evidence_out.as_deref(),
                args.benchmark_gate,
                dogfood_file_fingerprint_json(&plan.ledger_path),
                first_run_error.as_ref().map(|error| error.to_string()),
                benchmark_gate_error.as_ref().map(|error| error.to_string()),
            ),
        )?;
        println!("evidence_summary: {evidence_out}");
    }

    if let Some(error) = first_run_error {
        return Err(error);
    }
    if let Some(error) = benchmark_gate_error {
        return Err(error);
    }
    Ok(())
}

fn live_evidence_command(args: DogfoodLiveEvidenceArgs) -> AppResult<()> {
    let file = args.file.as_deref().ok_or_else(|| {
        app_error("dogfood live-evidence requires --file <path> to verify an evidence summary")
    })?;
    let raw = fs::read_to_string(file).map_err(|error| {
        app_error(format!(
            "failed to read dogfood live evidence file {file}: {error}"
        ))
    })?;
    let root = parse_root_object(&raw).map_err(|error| {
        app_error(format!(
            "failed to parse dogfood live evidence {file}: {error}"
        ))
    })?;
    let mut failures = live_evidence_failures(&root, &args);
    let report_gate_failures = if args.require_report_gate {
        match live_evidence_report_gate_failures(&root) {
            Ok(failures) => failures,
            Err(error) => vec![format!("report gate check failed: {error}")],
        }
    } else {
        Vec::new()
    };
    failures.extend(
        report_gate_failures
            .iter()
            .map(|failure| format!("report gate: {failure}")),
    );
    let result = live_evidence_verification_json(
        file,
        &root,
        &failures,
        args.require_report_gate,
        &report_gate_failures,
    );
    if let Some(out) = args.out.as_deref() {
        write_dogfood_json_artifact(out, &result, "dogfood live evidence verification")?;
    }

    if args.json {
        println!("{}", json_value_to_string(&result));
    } else if failures.is_empty() {
        println!("DeepSeekCode dogfood live evidence: pass");
        println!("file: {file}");
        if let Some(out) = args.out.as_deref() {
            println!("verification: {out}");
        }
        if let Some(value) = live_evidence_u64(&root, "appended_model_backed_records") {
            println!("appended_model_backed_records: {value}");
        }
        if let Some(command) = live_evidence_string(&root, "post_run_report_command") {
            println!("post_run_report_command: {command}");
        }
    } else {
        println!("DeepSeekCode dogfood live evidence: fail");
        println!("file: {file}");
        if let Some(out) = args.out.as_deref() {
            println!("verification: {out}");
        }
        for failure in &failures {
            println!("- {failure}");
        }
    }

    if failures.is_empty() {
        Ok(())
    } else {
        Err(app_error(format!(
            "dogfood live evidence failed:\n- {}",
            failures.join("\n- ")
        )))
    }
}

fn external_evidence_command(args: DogfoodExternalEvidenceArgs) -> AppResult<()> {
    let file = args.file.as_deref().ok_or_else(|| {
        app_error("dogfood external-evidence requires --file <path> to verify an evidence summary")
    })?;
    let raw = fs::read_to_string(file).map_err(|error| {
        app_error(format!(
            "failed to read dogfood external fixture evidence file {file}: {error}"
        ))
    })?;
    let root = parse_root_object(&raw).map_err(|error| {
        app_error(format!(
            "failed to parse dogfood external fixture evidence {file}: {error}"
        ))
    })?;
    let mut failures = external_evidence_failures(&root, &args);
    let ledger_match_failures = if args.require_ledger_match {
        match external_evidence_ledger_match_failures(&root) {
            Ok(failures) => failures,
            Err(error) => vec![format!("ledger match check failed: {error}")],
        }
    } else {
        Vec::new()
    };
    failures.extend(
        ledger_match_failures
            .iter()
            .map(|failure| format!("ledger match: {failure}")),
    );
    let result = external_evidence_verification_json(
        file,
        &root,
        &failures,
        args.require_ledger_match,
        &ledger_match_failures,
    );
    if let Some(out) = args.out.as_deref() {
        write_dogfood_json_artifact(
            out,
            &result,
            "dogfood external fixture evidence verification",
        )?;
    }

    if args.json {
        println!("{}", json_value_to_string(&result));
    } else if failures.is_empty() {
        println!("DeepSeekCode dogfood external fixture evidence: pass");
        println!("file: {file}");
        if let Some(out) = args.out.as_deref() {
            println!("verification: {out}");
        }
        if let Some(value) = live_evidence_u64(&root, "appended_successful_external_write_fixtures")
        {
            println!("appended_successful_external_write_fixtures: {value}");
        }
    } else {
        println!("DeepSeekCode dogfood external fixture evidence: fail");
        println!("file: {file}");
        if let Some(out) = args.out.as_deref() {
            println!("verification: {out}");
        }
        for failure in &failures {
            println!("- {failure}");
        }
    }

    if failures.is_empty() {
        Ok(())
    } else {
        Err(app_error(format!(
            "dogfood external fixture evidence failed:\n- {}",
            failures.join("\n- ")
        )))
    }
}

fn live_evidence_failures(
    root: &BTreeMap<String, JsonValue>,
    args: &DogfoodLiveEvidenceArgs,
) -> Vec<String> {
    let mut failures = Vec::new();
    match live_evidence_string(root, "kind") {
        Some("deepseek.dogfood.live_run_evidence.v1") => {}
        Some(other) => failures.push(format!(
            "unexpected evidence kind `{other}`, expected deepseek.dogfood.live_run_evidence.v1"
        )),
        None => failures.push("missing evidence kind".to_string()),
    }
    if args.require_completed && live_evidence_bool(root, "completed") != Some(true) {
        failures.push("live evidence is not completed".to_string());
    }
    if args.require_online {
        if live_evidence_string(root, "model_transport") != Some(MODEL_TRANSPORT_ONLINE) {
            failures.push("model_transport is not online".to_string());
        }
        if live_evidence_bool(root, "online_ready") != Some(true) {
            failures.push("online_ready is not true".to_string());
        }
    }
    if let Some(required) = args.require_appended_model_backed {
        let actual = live_evidence_u64(root, "appended_model_backed_records").unwrap_or(0);
        if actual < required as u64 {
            failures.push(format!(
                "appended model-backed records {actual} below required {required}"
            ));
        }
    }
    match live_evidence_cases(root) {
        Some(cases) if cases.is_empty() => {
            failures.push("evidence has no case records".to_string())
        }
        Some(cases) => {
            for (index, case) in cases.iter().enumerate() {
                let Some(case_root) = live_evidence_object(case) else {
                    failures.push(format!("case evidence #{index} is not an object"));
                    continue;
                };
                let appended = live_evidence_u64(case_root, "ledger_records_appended").unwrap_or(0);
                let model_backed = live_evidence_bool(case_root, "model_backed").unwrap_or(false);
                if appended > 0 && !model_backed {
                    failures.push(format!(
                        "case evidence #{index} appended ledger records without model_backed=true"
                    ));
                }
                if args.require_completed
                    && !matches!(case_root.get("error"), Some(JsonValue::Null))
                {
                    failures.push(format!("case evidence #{index} has a run error"));
                }
            }
        }
        None => failures.push("missing case evidence array".to_string()),
    }
    if args.require_benchmark_gate {
        let passed = root
            .get("benchmark_gate")
            .and_then(live_evidence_object)
            .and_then(|gate| live_evidence_bool(gate, "passed"));
        if passed != Some(true) {
            failures.push("benchmark gate did not pass".to_string());
        }
    }
    let report_command = live_evidence_string(root, "post_run_report_command").unwrap_or("");
    if !report_command.contains("--require-live-runs")
        || !report_command.contains("--require-live-category")
    {
        failures.push("post_run_report_command is missing live evidence gates".to_string());
    }
    if args.require_loop_surface_gate {
        if !report_command.contains("--require-live-category mcp:") {
            failures
                .push("post_run_report_command is missing MCP loop-surface live gate".to_string());
        }
        if !live_evidence_has_mcp_loop_surface_case(root) {
            failures.push("live evidence has no MCP loop-surface case".to_string());
        }
    }
    failures
}

fn external_evidence_failures(
    root: &BTreeMap<String, JsonValue>,
    args: &DogfoodExternalEvidenceArgs,
) -> Vec<String> {
    let mut failures = Vec::new();
    match live_evidence_string(root, "kind") {
        Some("deepseek.dogfood.external_fixture_evidence.v1") => {}
        Some(other) => failures.push(format!(
            "unexpected evidence kind `{other}`, expected deepseek.dogfood.external_fixture_evidence.v1"
        )),
        None => failures.push("missing evidence kind".to_string()),
    }
    if args.require_completed && live_evidence_bool(root, "completed") != Some(true) {
        failures.push("external fixture evidence is not completed".to_string());
    }
    if args.require_online {
        if live_evidence_string(root, "model_transport") != Some(MODEL_TRANSPORT_ONLINE) {
            failures.push("model_transport is not online".to_string());
        }
        if live_evidence_bool(root, "online_ready") != Some(true) {
            failures.push("online_ready is not true".to_string());
        }
    }
    if live_evidence_bool(root, "release_evidence_ready") != Some(true)
        && args.require_successful_external_fixtures.is_some()
    {
        failures.push("release_evidence_ready is not true".to_string());
    }
    if args.require_successful_external_fixtures.is_some() {
        if live_evidence_string(root, "post_validation_command")
            .map(str::trim)
            .filter(|command| !command.is_empty())
            .is_none()
        {
            failures.push("post_validation_command is missing".to_string());
        }
        if live_evidence_bool(root, "post_validation_passed") != Some(true) {
            failures.push("post_validation_passed is not true".to_string());
        }
    }
    if let Some(required) = args.require_successful_external_fixtures {
        let actual =
            live_evidence_u64(root, "appended_successful_external_write_fixtures").unwrap_or(0);
        if actual < required as u64 {
            failures.push(format!(
                "successful external write fixtures {actual} below required {required}"
            ));
        }
    }
    match external_evidence_records(root) {
        Some(records) if records.is_empty() => failures.push("evidence has no records".to_string()),
        Some(records) => {
            let computed_external = records
                .iter()
                .filter_map(live_evidence_object)
                .filter(|record| external_evidence_record_is_external_fixture(record))
                .count() as u64;
            let computed_success = records
                .iter()
                .filter_map(live_evidence_object)
                .filter(|record| {
                    external_evidence_record_is_successful_external_fixture(record)
                        || (live_evidence_bool(root, "post_validation_passed") == Some(true)
                            && external_evidence_record_is_external_fixture(record)
                            && live_evidence_bool(record, "model_backed") == Some(true))
                })
                .count() as u64;
            if live_evidence_u64(root, "appended_external_write_fixtures").unwrap_or(0)
                != computed_external
            {
                failures
                    .push("appended_external_write_fixtures does not match records".to_string());
            }
            if live_evidence_u64(root, "appended_successful_external_write_fixtures").unwrap_or(0)
                != computed_success
            {
                failures.push(
                    "appended_successful_external_write_fixtures does not match records"
                        .to_string(),
                );
            }
            for (index, record) in records.iter().enumerate() {
                let Some(record_root) = live_evidence_object(record) else {
                    failures.push(format!(
                        "external evidence record #{index} is not an object"
                    ));
                    continue;
                };
                if !external_evidence_record_is_external_fixture(record_root) {
                    failures.push(format!(
                        "external evidence record #{index} is not marked external-write-fixture"
                    ));
                }
                if args.require_online
                    && live_evidence_string(record_root, "model_transport")
                        != Some(MODEL_TRANSPORT_ONLINE)
                {
                    failures.push(format!(
                        "external evidence record #{index} is not online model-backed"
                    ));
                }
            }
        }
        None => failures.push("missing external fixture evidence records array".to_string()),
    }
    failures
}

fn external_evidence_ledger_match_failures(
    root: &BTreeMap<String, JsonValue>,
) -> AppResult<Vec<String>> {
    let ledger = live_evidence_string(root, "ledger")
        .ok_or_else(|| app_error("external evidence missing ledger path"))?;
    let ledger_path = Path::new(ledger);
    let records = load_records(ledger_path)?;
    let mut failures = live_evidence_ledger_fingerprint_failures(root, ledger_path);
    let Some(evidence_records) = external_evidence_records(root) else {
        failures.push("missing external fixture evidence records array".to_string());
        return Ok(failures);
    };
    for (index, record) in evidence_records.iter().enumerate() {
        let Some(record_root) = live_evidence_object(record) else {
            failures.push(format!(
                "external evidence record #{index} is not an object"
            ));
            continue;
        };
        if !live_evidence_case_matches_any_record(record_root, &records) {
            let timestamp = live_evidence_u64(record_root, "timestamp_secs")
                .map(|value| value.to_string())
                .unwrap_or_else(|| "null".to_string());
            let outcome = live_evidence_string(record_root, "outcome").unwrap_or("null");
            let transport = live_evidence_string(record_root, "model_transport").unwrap_or("null");
            failures.push(format!(
                "external evidence record #{index} was not found in ledger (timestamp_secs={timestamp}, outcome={outcome}, model_transport={transport})"
            ));
        }
    }
    Ok(failures)
}

fn live_evidence_verification_json(
    file: &str,
    root: &BTreeMap<String, JsonValue>,
    failures: &[String],
    report_gate_required: bool,
    report_gate_failures: &[String],
) -> JsonValue {
    let mut out = BTreeMap::new();
    out.insert(
        "kind".to_string(),
        JsonValue::String("deepseek.dogfood.live_evidence_verification.v1".to_string()),
    );
    out.insert("file".to_string(), JsonValue::String(file.to_string()));
    out.insert("ok".to_string(), JsonValue::Bool(failures.is_empty()));
    out.insert(
        "failures".to_string(),
        JsonValue::Array(failures.iter().cloned().map(JsonValue::String).collect()),
    );
    out.insert(
        "completed".to_string(),
        live_evidence_bool(root, "completed")
            .map(JsonValue::Bool)
            .unwrap_or(JsonValue::Null),
    );
    out.insert(
        "online_ready".to_string(),
        live_evidence_bool(root, "online_ready")
            .map(JsonValue::Bool)
            .unwrap_or(JsonValue::Null),
    );
    out.insert(
        "model_transport".to_string(),
        live_evidence_string(root, "model_transport")
            .map(|value| JsonValue::String(value.to_string()))
            .unwrap_or(JsonValue::Null),
    );
    out.insert(
        "appended_model_backed_records".to_string(),
        live_evidence_u64(root, "appended_model_backed_records")
            .map(|value| JsonValue::Number(value.to_string()))
            .unwrap_or(JsonValue::Null),
    );
    out.insert(
        "case_count".to_string(),
        live_evidence_cases(root)
            .map(|cases| JsonValue::Number(cases.len().to_string()))
            .unwrap_or(JsonValue::Null),
    );
    out.insert(
        "benchmark_gate_passed".to_string(),
        root.get("benchmark_gate")
            .and_then(live_evidence_object)
            .and_then(|gate| live_evidence_bool(gate, "passed"))
            .map(JsonValue::Bool)
            .unwrap_or(JsonValue::Null),
    );
    out.insert(
        "report_gate_required".to_string(),
        JsonValue::Bool(report_gate_required),
    );
    out.insert(
        "loop_surface_case_present".to_string(),
        JsonValue::Bool(live_evidence_has_mcp_loop_surface_case(root)),
    );
    out.insert(
        "report_gate_passed".to_string(),
        JsonValue::Bool(report_gate_required && report_gate_failures.is_empty()),
    );
    out.insert(
        "report_gate_failures".to_string(),
        JsonValue::Array(
            report_gate_failures
                .iter()
                .cloned()
                .map(JsonValue::String)
                .collect(),
        ),
    );
    out.insert(
        "post_run_report_command".to_string(),
        live_evidence_string(root, "post_run_report_command")
            .map(|value| JsonValue::String(value.to_string()))
            .unwrap_or(JsonValue::Null),
    );
    out.insert(
        "ledger_fingerprint".to_string(),
        root.get("ledger_fingerprint")
            .cloned()
            .unwrap_or(JsonValue::Null),
    );
    out.insert(
        "current_ledger_fingerprint".to_string(),
        if report_gate_required {
            live_evidence_string(root, "ledger")
                .map(|ledger| dogfood_file_fingerprint_json(Path::new(ledger)))
                .unwrap_or(JsonValue::Null)
        } else {
            JsonValue::Null
        },
    );
    JsonValue::Object(out)
}

fn external_evidence_verification_json(
    file: &str,
    root: &BTreeMap<String, JsonValue>,
    failures: &[String],
    ledger_match_required: bool,
    ledger_match_failures: &[String],
) -> JsonValue {
    let mut out = BTreeMap::new();
    out.insert(
        "kind".to_string(),
        JsonValue::String("deepseek.dogfood.external_fixture_evidence_verification.v1".to_string()),
    );
    out.insert("file".to_string(), JsonValue::String(file.to_string()));
    out.insert("ok".to_string(), JsonValue::Bool(failures.is_empty()));
    out.insert(
        "failures".to_string(),
        JsonValue::Array(failures.iter().cloned().map(JsonValue::String).collect()),
    );
    out.insert(
        "completed".to_string(),
        live_evidence_bool(root, "completed")
            .map(JsonValue::Bool)
            .unwrap_or(JsonValue::Null),
    );
    out.insert(
        "online_ready".to_string(),
        live_evidence_bool(root, "online_ready")
            .map(JsonValue::Bool)
            .unwrap_or(JsonValue::Null),
    );
    out.insert(
        "model_transport".to_string(),
        live_evidence_string(root, "model_transport")
            .map(|value| JsonValue::String(value.to_string()))
            .unwrap_or(JsonValue::Null),
    );
    out.insert(
        "release_evidence_ready".to_string(),
        live_evidence_bool(root, "release_evidence_ready")
            .map(JsonValue::Bool)
            .unwrap_or(JsonValue::Null),
    );
    out.insert(
        "post_validation_command".to_string(),
        live_evidence_string(root, "post_validation_command")
            .map(|value| JsonValue::String(value.to_string()))
            .unwrap_or(JsonValue::Null),
    );
    out.insert(
        "post_validation_passed".to_string(),
        live_evidence_bool(root, "post_validation_passed")
            .map(JsonValue::Bool)
            .unwrap_or(JsonValue::Null),
    );
    out.insert(
        "appended_model_backed_records".to_string(),
        live_evidence_u64(root, "appended_model_backed_records")
            .map(|value| JsonValue::Number(value.to_string()))
            .unwrap_or(JsonValue::Null),
    );
    out.insert(
        "appended_external_write_fixtures".to_string(),
        live_evidence_u64(root, "appended_external_write_fixtures")
            .map(|value| JsonValue::Number(value.to_string()))
            .unwrap_or(JsonValue::Null),
    );
    out.insert(
        "appended_successful_external_write_fixtures".to_string(),
        live_evidence_u64(root, "appended_successful_external_write_fixtures")
            .map(|value| JsonValue::Number(value.to_string()))
            .unwrap_or(JsonValue::Null),
    );
    out.insert(
        "record_count".to_string(),
        external_evidence_records(root)
            .map(|records| JsonValue::Number(records.len().to_string()))
            .unwrap_or(JsonValue::Null),
    );
    out.insert(
        "ledger_match_required".to_string(),
        JsonValue::Bool(ledger_match_required),
    );
    out.insert(
        "ledger_match_passed".to_string(),
        JsonValue::Bool(ledger_match_required && ledger_match_failures.is_empty()),
    );
    out.insert(
        "ledger_match_failures".to_string(),
        JsonValue::Array(
            ledger_match_failures
                .iter()
                .cloned()
                .map(JsonValue::String)
                .collect(),
        ),
    );
    out.insert(
        "ledger_fingerprint".to_string(),
        root.get("ledger_fingerprint")
            .cloned()
            .unwrap_or(JsonValue::Null),
    );
    out.insert(
        "current_ledger_fingerprint".to_string(),
        if ledger_match_required {
            live_evidence_string(root, "ledger")
                .map(|ledger| dogfood_file_fingerprint_json(Path::new(ledger)))
                .unwrap_or(JsonValue::Null)
        } else {
            JsonValue::Null
        },
    );
    JsonValue::Object(out)
}

fn live_evidence_report_gate_failures(
    root: &BTreeMap<String, JsonValue>,
) -> AppResult<Vec<String>> {
    let ledger = live_evidence_string(root, "ledger")
        .ok_or_else(|| app_error("live evidence missing ledger path"))?;
    let records = load_records(Path::new(ledger))?;
    let args = live_evidence_report_gate_args(root)?;
    let mut failures = report_requirement_failures(&records, &args);
    failures.extend(live_evidence_ledger_fingerprint_failures(
        root,
        Path::new(ledger),
    ));
    failures.extend(live_evidence_ledger_match_failures(root, &records));
    Ok(failures)
}

fn live_evidence_report_gate_args(
    root: &BTreeMap<String, JsonValue>,
) -> AppResult<DogfoodReportArgs> {
    let gate = root
        .get("evidence_gate")
        .and_then(live_evidence_object)
        .ok_or_else(|| app_error("live evidence missing evidence_gate object"))?;
    let mut args = DogfoodReportArgs::default();
    args.require_live_runs = Some(read_live_evidence_usize(
        gate,
        "require_live_runs",
        "evidence_gate",
    )?);
    args.require_live_success_rate = Some(read_live_evidence_f64(
        gate,
        "require_live_success_rate",
        "evidence_gate",
    )?);
    let categories = gate
        .get("require_live_categories")
        .and_then(json_as_array)
        .ok_or_else(|| app_error("evidence_gate missing require_live_categories array"))?;
    for (index, value) in categories.iter().enumerate() {
        let category = live_evidence_object(value).ok_or_else(|| {
            app_error(format!(
                "evidence_gate require_live_categories[{index}] must be an object"
            ))
        })?;
        let category_name = live_evidence_string(category, "category").ok_or_else(|| {
            app_error(format!(
                "evidence_gate require_live_categories[{index}] missing category"
            ))
        })?;
        args.require_live_categories
            .push(DogfoodCategoryRequirement {
                category: category_name.to_string(),
                min_runs: read_live_evidence_usize(
                    category,
                    "min_runs",
                    "evidence_gate require_live_categories",
                )?,
                min_success_percent: read_live_evidence_f64(
                    category,
                    "min_success_rate",
                    "evidence_gate require_live_categories",
                )?,
            });
    }
    Ok(args)
}

fn read_live_evidence_usize(
    root: &BTreeMap<String, JsonValue>,
    key: &str,
    context: &str,
) -> AppResult<usize> {
    let value = live_evidence_u64(root, key)
        .ok_or_else(|| app_error(format!("{context} missing numeric {key}")))?;
    usize::try_from(value).map_err(|_| app_error(format!("{context} {key} is too large")))
}

fn read_live_evidence_f64(
    root: &BTreeMap<String, JsonValue>,
    key: &str,
    context: &str,
) -> AppResult<f64> {
    let Some(JsonValue::Number(value)) = root.get(key) else {
        return Err(app_error(format!("{context} missing numeric {key}")));
    };
    value
        .parse::<f64>()
        .map_err(|_| app_error(format!("{context} {key} is not a valid number")))
}

fn live_evidence_ledger_match_failures(
    root: &BTreeMap<String, JsonValue>,
    records: &[DogfoodRecord],
) -> Vec<String> {
    let mut failures = Vec::new();
    let summary_model_backed =
        live_evidence_u64(root, "appended_model_backed_records").unwrap_or(0);
    let Some(cases) = live_evidence_cases(root) else {
        return failures;
    };
    let mut case_model_backed_total = 0u64;
    for (index, case) in cases.iter().enumerate() {
        let Some(case_root) = live_evidence_object(case) else {
            continue;
        };
        let appended = live_evidence_u64(case_root, "ledger_records_appended").unwrap_or(0);
        if appended == 0 {
            continue;
        }
        let model_backed_appended =
            live_evidence_u64(case_root, "model_backed_records_appended").unwrap_or(0);
        case_model_backed_total = case_model_backed_total.saturating_add(model_backed_appended);
        if live_evidence_case_matches_any_record(case_root, records) {
            continue;
        }
        let name = live_evidence_string(case_root, "name").unwrap_or("unknown");
        let timestamp = live_evidence_u64(case_root, "timestamp_secs")
            .map(|value| value.to_string())
            .unwrap_or_else(|| "null".to_string());
        let outcome = live_evidence_string(case_root, "outcome").unwrap_or("null");
        let transport = live_evidence_string(case_root, "model_transport").unwrap_or("null");
        failures.push(format!(
            "case evidence #{index} `{name}` was not found in ledger (timestamp_secs={timestamp}, outcome={outcome}, model_transport={transport})"
        ));
    }
    if case_model_backed_total != summary_model_backed {
        failures.push(format!(
            "case model-backed total {case_model_backed_total} does not match summary appended_model_backed_records {summary_model_backed}"
        ));
    }
    failures
}

fn live_evidence_ledger_fingerprint_failures(
    root: &BTreeMap<String, JsonValue>,
    ledger: &Path,
) -> Vec<String> {
    let mut failures = Vec::new();
    let Some(expected_value) = root.get("ledger_fingerprint") else {
        failures.push("missing ledger_fingerprint".to_string());
        return failures;
    };
    let Some(expected) = live_evidence_object(expected_value) else {
        failures.push("ledger_fingerprint is not an object".to_string());
        return failures;
    };
    let current_value = dogfood_file_fingerprint_json(ledger);
    let Some(current) = live_evidence_object(&current_value) else {
        failures.push("current ledger fingerprint is not an object".to_string());
        return failures;
    };
    if live_evidence_bool(expected, "ok") != Some(true) {
        failures.push("ledger_fingerprint is not ok".to_string());
        return failures;
    }
    if live_evidence_bool(current, "ok") != Some(true) {
        let error = live_evidence_string(current, "error").unwrap_or("unknown error");
        failures.push(format!("failed to fingerprint current ledger: {error}"));
        return failures;
    }
    for key in ["algorithm", "path", "fnv1a64"] {
        let expected_value = live_evidence_string(expected, key).unwrap_or("");
        let current_value = live_evidence_string(current, key).unwrap_or("");
        if expected_value != current_value {
            failures.push(format!(
                "ledger_fingerprint {key} mismatch: evidence={expected_value}, current={current_value}"
            ));
        }
    }
    let expected_bytes = live_evidence_u64(expected, "bytes");
    let current_bytes = live_evidence_u64(current, "bytes");
    if expected_bytes != current_bytes {
        failures.push(format!(
            "ledger_fingerprint bytes mismatch: evidence={}, current={}",
            expected_bytes
                .map(|value| value.to_string())
                .unwrap_or_else(|| "null".to_string()),
            current_bytes
                .map(|value| value.to_string())
                .unwrap_or_else(|| "null".to_string())
        ));
    }
    failures
}

fn live_evidence_case_matches_any_record(
    case_root: &BTreeMap<String, JsonValue>,
    records: &[DogfoodRecord],
) -> bool {
    let Some(timestamp_secs) = live_evidence_u64(case_root, "timestamp_secs") else {
        return false;
    };
    let Some(outcome) = live_evidence_string(case_root, "outcome") else {
        return false;
    };
    let Some(model_transport) = live_evidence_string(case_root, "model_transport") else {
        return false;
    };
    let model_backed = live_evidence_bool(case_root, "model_backed").unwrap_or(false);
    let category = live_evidence_string(case_root, "benchmark_category");
    records.iter().any(|record| {
        record.timestamp_secs == timestamp_secs
            && record.outcome.label() == outcome
            && record.model_transport == model_transport
            && record_is_model_backed(record) == model_backed
            && category
                .is_none_or(|category| record.benchmark_category.as_deref() == Some(category))
    })
}

fn live_evidence_object(value: &JsonValue) -> Option<&BTreeMap<String, JsonValue>> {
    match value {
        JsonValue::Object(value) => Some(value),
        _ => None,
    }
}

fn live_evidence_cases(root: &BTreeMap<String, JsonValue>) -> Option<&Vec<JsonValue>> {
    root.get("cases").and_then(json_as_array)
}

fn live_evidence_has_mcp_loop_surface_case(root: &BTreeMap<String, JsonValue>) -> bool {
    live_evidence_cases(root)
        .map(|cases| {
            cases
                .iter()
                .filter_map(live_evidence_object)
                .any(|case| live_evidence_string(case, "benchmark_category") == Some("mcp"))
        })
        .unwrap_or(false)
}

fn external_evidence_records(root: &BTreeMap<String, JsonValue>) -> Option<&Vec<JsonValue>> {
    root.get("records").and_then(json_as_array)
}

fn external_evidence_record_is_external_fixture(record: &BTreeMap<String, JsonValue>) -> bool {
    live_evidence_string(record, "notes")
        .is_some_and(|notes| notes.contains("external-write-fixture"))
        && live_evidence_string(record, "benchmark_category") == Some("write_validate")
}

fn external_evidence_record_is_successful_external_fixture(
    record: &BTreeMap<String, JsonValue>,
) -> bool {
    external_evidence_record_is_external_fixture(record)
        && live_evidence_string(record, "model_transport") == Some(MODEL_TRANSPORT_ONLINE)
        && live_evidence_string(record, "outcome") == Some(DogfoodOutcome::Success.label())
}

fn live_evidence_string<'a>(root: &'a BTreeMap<String, JsonValue>, key: &str) -> Option<&'a str> {
    root.get(key).and_then(json_as_string)
}

fn live_evidence_u64(root: &BTreeMap<String, JsonValue>, key: &str) -> Option<u64> {
    root.get(key).and_then(json_as_u64)
}

fn live_evidence_bool(root: &BTreeMap<String, JsonValue>, key: &str) -> Option<bool> {
    match root.get(key) {
        Some(JsonValue::Bool(value)) => Some(*value),
        _ => None,
    }
}

struct EnvVarGuard {
    name: String,
    previous: Option<std::ffi::OsString>,
}

impl Drop for EnvVarGuard {
    fn drop(&mut self) {
        match self.previous.as_ref() {
            Some(value) => unsafe { env::set_var(&self.name, value) },
            None => unsafe { env::remove_var(&self.name) },
        }
    }
}

fn load_live_run_api_key_file(
    config: &crate::config::types::AppConfig,
    path: &str,
) -> AppResult<EnvVarGuard> {
    let api_key_env = config.model.api_key_env.trim();
    if api_key_env.is_empty() || api_key_env.to_ascii_uppercase().contains("OFFLINE") {
        return Err(app_error(
            "dogfood live-run --api-key-file requires model.api_key_env to name a real provider environment variable",
        ));
    }
    let path = PathBuf::from(path);
    validate_live_run_api_key_file_path(&path)?;
    let key = fs::read_to_string(&path).map_err(|error| {
        app_error(format!(
            "failed to read dogfood live-run API key file {}: {error}",
            path.display()
        ))
    })?;
    let key = key.trim();
    if key.is_empty() {
        return Err(app_error(format!(
            "dogfood live-run API key file is empty: {}",
            path.display()
        )));
    }
    let guard = EnvVarGuard {
        name: api_key_env.to_string(),
        previous: env::var_os(api_key_env),
    };
    unsafe {
        env::set_var(api_key_env, key);
    }
    Ok(guard)
}

fn validate_live_run_api_key_file_path(path: &Path) -> AppResult<()> {
    if !path.is_file() {
        return Err(app_error(format!(
            "dogfood live-run API key file is missing or not a file: {}",
            path.display()
        )));
    }
    let repo_root = env::current_dir()?;
    let repo_root = fs::canonicalize(&repo_root).map_err(|error| {
        app_error(format!(
            "failed to canonicalize repository root {}: {error}",
            repo_root.display()
        ))
    })?;
    let key_path = fs::canonicalize(path).map_err(|error| {
        app_error(format!(
            "failed to canonicalize dogfood live-run API key file {}: {error}",
            path.display()
        ))
    })?;
    if key_path.starts_with(&repo_root) {
        return Err(app_error(format!(
            "dogfood live-run API key file must live outside the repository: {}",
            key_path.display()
        )));
    }
    Ok(())
}

fn repair_cache_evidence_command(
    config: &crate::config::types::AppConfig,
    args: DogfoodRepairCacheEvidenceArgs,
) -> AppResult<()> {
    let repo_root = std::env::current_dir()?;
    let runtime_root = PathBuf::from(&config.workspace.config_dir).join("runtime");
    let store = RuntimeStore::new(runtime_root.clone());
    let out_path = args.out.map(PathBuf::from).unwrap_or_else(|| {
        config
            .workspace
            .dogfood_dir()
            .join("repair-cache-evidence.json")
    });
    let evidence = repair_cache_evidence_summary_json(
        &store,
        &runtime_root,
        &repo_root,
        "deepseek-v4-flash",
        "src/model/tool_repair.rs",
    )?;
    write_dogfood_json_artifact(
        out_path.to_string_lossy().as_ref(),
        &evidence,
        "dogfood repair/cache evidence summary",
    )?;

    if args.json {
        println!("{}", json_value_to_string(&evidence));
        return Ok(());
    }

    println!("DeepSeekCode dogfood repair/cache evidence");
    println!("runtime: {}", runtime_root.display());
    println!("evidence: {}", out_path.display());
    if let Some(root) = json_as_object(&evidence) {
        if let (Some(before), Some(after)) = (
            root.get("before_thread_id").and_then(json_as_string),
            root.get("after_thread_id").and_then(json_as_string),
        ) {
            println!("before_thread: {before}");
            println!("after_thread: {after}");
            println!("diff: deepseek events diff {before} {after}");
            println!("after_stats: deepseek stats --thread {after}");
        }
    }
    Ok(())
}

fn repair_cache_evidence_summary_json(
    store: &RuntimeStore,
    runtime_root: &Path,
    workspace: &Path,
    model: &str,
    read_path: &str,
) -> AppResult<JsonValue> {
    let model = if model.trim().is_empty() {
        "deepseek-v4-flash"
    } else {
        model.trim()
    };
    let session = store.create_session(
        "Repair/cache dogfood evidence".to_string(),
        workspace.display().to_string(),
    )?;
    let before = store.create_thread_for_session(
        &session.id,
        "Before repair: malformed DeepSeek tool call".to_string(),
        workspace.display().to_string(),
        model.to_string(),
        "dogfood".to_string(),
    )?;
    let after = store.create_thread_for_session(
        &session.id,
        "After repair: recovered DeepSeek tool call".to_string(),
        workspace.display().to_string(),
        model.to_string(),
        "dogfood".to_string(),
    )?;

    let raw_arguments = format!("{{\"path\":\"{}\"", json_escape(read_path));
    let strict_parse_error = strict_json_object_complete_error(&raw_arguments)
        .unwrap_or_else(|| "strict parser unexpectedly accepted malformed input".to_string());

    let before_turn = store.append_turn(
        &before.id,
        "assistant".to_string(),
        format!("DeepSeek emitted malformed read_file arguments: {raw_arguments}"),
    )?;
    store.append_item(
        &before.id,
        Some(&before_turn.id),
        "tool_result".to_string(),
        None,
        format!("tool_call_parse_failed: {strict_parse_error}"),
        "failed".to_string(),
    )?;
    let before_usage = store.append_usage_with_cache(
        &before.id,
        Some(&before_turn.id),
        model.to_string(),
        "dogfood-repair-cache-before".to_string(),
        1_200,
        120,
        0,
        1_200,
    )?;
    let before_prompt_layers = vec![repair_cache_prompt_snapshot(1, 1_200)];
    store.append_thread_event(
        &before.id,
        "prompt_layers_recorded",
        prompt_layers_event_payload(&before_turn.id, &before_usage.id, &before_prompt_layers),
    )?;

    let (repaired_args, repair_note) = parse_tool_arguments_with_repair(&raw_arguments)?;
    let repair_note = repair_note.ok_or_else(|| {
        app_error("dogfood repair/cache evidence expected truncated-json repair note")
    })?;
    let tool_output = ReadFileTool.execute(ToolInput {
        args: repaired_args.clone(),
    })?;
    let tool_result_ok = tool_output.summary.contains("ToolRepairNote")
        || tool_output
            .summary
            .contains("parse_tool_arguments_with_repair");

    let after_turn = store.append_turn(
        &after.id,
        "assistant".to_string(),
        format!(
            "DeepSeek emitted the same malformed read_file arguments; repair recovered them: {raw_arguments}"
        ),
    )?;
    store.append_item(
        &after.id,
        Some(&after_turn.id),
        "tool_call".to_string(),
        None,
        format!(
            "read_file {}",
            json_value_to_string(&string_map_to_json(&repaired_args))
        ),
        "completed".to_string(),
    )?;
    store.append_thread_event(
        &after.id,
        "tool_call_repair",
        json_object([
            ("type", JsonValue::String("tool_call_repair".to_string())),
            ("kind", JsonValue::String(repair_note.kind.to_string())),
            ("detail", JsonValue::String(repair_note.detail.clone())),
            ("tool_name", JsonValue::String("read_file".to_string())),
            ("raw_arguments", JsonValue::String(raw_arguments.clone())),
        ]),
    )?;
    store.append_item(
        &after.id,
        Some(&after_turn.id),
        "tool_result".to_string(),
        None,
        clip(&tool_output.summary, 1_000),
        if tool_result_ok {
            "completed".to_string()
        } else {
            "failed".to_string()
        },
    )?;
    let after_usage = store.append_usage_with_cache(
        &after.id,
        Some(&after_turn.id),
        model.to_string(),
        "dogfood-repair-cache-after".to_string(),
        1_200,
        120,
        900,
        300,
    )?;
    let after_prompt_layers = vec![repair_cache_prompt_snapshot(1, 1_200)];
    store.append_thread_event(
        &after.id,
        "prompt_layers_recorded",
        prompt_layers_event_payload(&after_turn.id, &after_usage.id, &after_prompt_layers),
    )?;

    let before_events = store.read_events(&before.id, 0)?;
    let after_events = store.read_events(&after.id, 0)?;
    let before_repair_events = count_events(&before_events, "tool_call_repair");
    let after_repair_events = count_events(&after_events, "tool_call_repair");
    let before_prompt_layer_events = count_events(&before_events, "prompt_layers_recorded");
    let after_prompt_layer_events = count_events(&after_events, "prompt_layers_recorded");
    let before_hit_rate = cache_hit_basis_points(0, 1_200);
    let after_hit_rate = cache_hit_basis_points(900, 300);
    let session_id = session.id.clone();
    let before_thread_id = before.id.clone();
    let after_thread_id = after.id.clone();
    let repair_kind = repair_note.kind.to_string();
    let repair_detail = repair_note.detail.clone();

    Ok(json_object([
        (
            "kind",
            JsonValue::String("deepseek.dogfood.repair_cache_evidence.v1".to_string()),
        ),
        (
            "runtime_root",
            JsonValue::String(runtime_root.display().to_string()),
        ),
        ("session_id", JsonValue::String(session_id)),
        (
            "before_thread_id",
            JsonValue::String(before_thread_id.clone()),
        ),
        (
            "after_thread_id",
            JsonValue::String(after_thread_id.clone()),
        ),
        (
            "raw_trace",
            json_object([
                (
                    "provider",
                    JsonValue::String("deepseek-openai-compatible".to_string()),
                ),
                ("tool_name", JsonValue::String("read_file".to_string())),
                ("raw_arguments", JsonValue::String(raw_arguments)),
                ("strict_parse_error", JsonValue::String(strict_parse_error)),
                ("would_fail_without_repair", JsonValue::Bool(true)),
            ]),
        ),
        (
            "repair",
            json_object([
                ("completed", JsonValue::Bool(tool_result_ok)),
                ("kind", JsonValue::String(repair_kind)),
                ("detail", JsonValue::String(repair_detail)),
                ("arguments", string_map_to_json(&repaired_args)),
                (
                    "tool_result_excerpt",
                    JsonValue::String(clip(&tool_output.summary, 240)),
                ),
            ]),
        ),
        (
            "cache_comparison",
            json_object([
                (
                    "before_prompt_cache_hit_tokens",
                    JsonValue::Number("0".to_string()),
                ),
                (
                    "before_prompt_cache_miss_tokens",
                    JsonValue::Number("1200".to_string()),
                ),
                (
                    "before_prompt_cache_hit_basis_points",
                    JsonValue::Number(before_hit_rate.to_string()),
                ),
                (
                    "after_prompt_cache_hit_tokens",
                    JsonValue::Number("900".to_string()),
                ),
                (
                    "after_prompt_cache_miss_tokens",
                    JsonValue::Number("300".to_string()),
                ),
                (
                    "after_prompt_cache_hit_basis_points",
                    JsonValue::Number(after_hit_rate.to_string()),
                ),
                (
                    "hit_rate_delta_basis_points",
                    JsonValue::Number(after_hit_rate.saturating_sub(before_hit_rate).to_string()),
                ),
            ]),
        ),
        (
            "observable_events",
            json_object([
                (
                    "before_repair_events",
                    JsonValue::Number(before_repair_events.to_string()),
                ),
                (
                    "after_repair_events",
                    JsonValue::Number(after_repair_events.to_string()),
                ),
                (
                    "before_prompt_layer_events",
                    JsonValue::Number(before_prompt_layer_events.to_string()),
                ),
                (
                    "after_prompt_layer_events",
                    JsonValue::Number(after_prompt_layer_events.to_string()),
                ),
            ]),
        ),
        (
            "commands",
            json_array(vec![
                JsonValue::String(format!("deepseek events replay {after_thread_id}")),
                JsonValue::String(format!(
                    "deepseek events diff {before_thread_id} {after_thread_id}"
                )),
                JsonValue::String(format!("deepseek stats --thread {after_thread_id}")),
            ]),
        ),
        (
            "acceptance",
            json_object([
                (
                    "formerly_failing_trace_recovers",
                    JsonValue::Bool(tool_result_ok),
                ),
                (
                    "every_repaired_call_observable",
                    JsonValue::Bool(after_repair_events >= 1),
                ),
                (
                    "cache_diagnostics_visible",
                    JsonValue::Bool(
                        after_prompt_layer_events >= 1 && after_hit_rate > before_hit_rate,
                    ),
                ),
                (
                    "before_after_comparable",
                    JsonValue::Bool(
                        before_prompt_layer_events >= 1 && after_prompt_layer_events >= 1,
                    ),
                ),
            ]),
        ),
    ]))
}

fn strict_json_object_complete_error(raw: &str) -> Option<String> {
    let bytes = raw.trim().as_bytes();
    let mut index = 0;
    match parse_value(bytes, &mut index) {
        Ok(JsonValue::Object(_)) => {
            if bytes[index..]
                .iter()
                .any(|byte| !byte.is_ascii_whitespace())
            {
                Some("unexpected trailing json input".to_string())
            } else {
                None
            }
        }
        Ok(_) => Some("json root must be an object".to_string()),
        Err(error) => Some(error.to_string()),
    }
}

fn repair_cache_prompt_snapshot(step: usize, estimated_tokens: u64) -> PromptLayerSnapshot {
    let layers = vec![
        PromptLayerRecord {
            name: "system_static".to_string(),
            text_sha256: "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
                .to_string(),
            bytes: 2400,
            estimated_tokens: 600,
            cache_stable: true,
        },
        PromptLayerRecord {
            name: "tool_catalog".to_string(),
            text_sha256: "abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789"
                .to_string(),
            bytes: 1600,
            estimated_tokens: 400,
            cache_stable: true,
        },
        PromptLayerRecord {
            name: "append_only_turns".to_string(),
            text_sha256: "fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210"
                .to_string(),
            bytes: 800,
            estimated_tokens: estimated_tokens.saturating_sub(1_000),
            cache_stable: false,
        },
    ];
    PromptLayerSnapshot {
        step,
        total_bytes: layers.iter().map(|layer| layer.bytes).sum(),
        estimated_tokens,
        layers,
    }
}

fn string_map_to_json(values: &BTreeMap<String, String>) -> JsonValue {
    JsonValue::Object(
        values
            .iter()
            .map(|(key, value)| (key.clone(), JsonValue::String(value.clone())))
            .collect(),
    )
}

fn count_events(events: &[crate::core::runtime::RuntimeEvent], kind: &str) -> u64 {
    events.iter().filter(|event| event.kind == kind).count() as u64
}

fn cache_hit_basis_points(hit: u64, miss: u64) -> u64 {
    let total = hit.saturating_add(miss);
    if total == 0 {
        0
    } else {
        hit.saturating_mul(10_000) / total
    }
}

fn replay_benchmark_command(
    config: &crate::config::types::AppConfig,
    args: DogfoodReplayArgs,
) -> AppResult<()> {
    let manifest_path = args
        .manifest
        .map(PathBuf::from)
        .unwrap_or_else(|| config.workspace.benchmark_manifest_path());
    let summaries = crate::cli::commands::benchmark::load_manifest_case_summaries(&manifest_path)?;
    let selected = select_replayable_cases(&summaries, args.category.as_deref(), args.limit);

    println!("DeepSeekCode dogfood benchmark replay");
    println!("manifest: {}", manifest_path.display());
    println!(
        "selected: {}{}",
        selected.len(),
        args.category
            .as_deref()
            .map(|category| format!(" (category: {category})"))
            .unwrap_or_default()
    );

    if selected.is_empty() {
        println!("no replayable benchmark cases matched the requested filters");
        return Ok(());
    }

    for case in &selected {
        println!("replay: {}", case.name);
        run_live_task(
            config,
            DogfoodRunArgs {
                task: String::new(),
                from_benchmark: Some(case.name.clone()),
                benchmark_manifest: Some(manifest_path.display().to_string()),
                skill: None,
                budget: None,
                workdir: None,
                isolate_workdir: false,
                outcome: None,
                manual_intervention: false,
                benchmark_gate: false,
                notes: None,
            },
        )?;
    }

    if args.benchmark_gate {
        println!("post-replay benchmark gate: running default benchmark baseline");
        crate::cli::commands::benchmark::run_with_config(config.clone(), BenchmarkArgs::default())?;
    }
    Ok(())
}

fn export_benchmark_command(
    config: &crate::config::types::AppConfig,
    args: DogfoodExportArgs,
) -> AppResult<()> {
    let ledger_path = config.workspace.dogfood_ledger_path();
    let out_path = args
        .out
        .map(PathBuf::from)
        .unwrap_or_else(|| config.workspace.dogfood_benchmark_seed_path());
    let limit = args.limit.unwrap_or(DEFAULT_REPORT_LIMIT);
    let records = load_records(&ledger_path)?;
    let repo_root = std::env::current_dir()?;
    let export = render_benchmark_seed_export(&records, limit, args.outcome, &repo_root);
    if let Some(parent) = out_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&out_path, export)?;
    println!("DeepSeekCode dogfood benchmark export");
    println!("ledger: {}", ledger_path.display());
    println!("out: {}", out_path.display());
    Ok(())
}

fn promote_benchmark_command(
    config: &crate::config::types::AppConfig,
    args: DogfoodPromoteArgs,
) -> AppResult<()> {
    let ledger_path = config.workspace.dogfood_ledger_path();
    let manifest_path = args
        .manifest
        .map(PathBuf::from)
        .unwrap_or_else(|| config.workspace.benchmark_manifest_path());
    let limit = args.limit.unwrap_or(DEFAULT_REPORT_LIMIT);
    let records = load_records(&ledger_path)?;
    let existing = crate::cli::commands::benchmark::load_manifest_case_summaries(&manifest_path)?;
    let repo_root = std::env::current_dir()?;
    let plan = build_promotion_plan(&records, &existing, limit, args.outcome, &repo_root);

    println!("DeepSeekCode dogfood benchmark promotion");
    println!("ledger: {}", ledger_path.display());
    println!("manifest: {}", manifest_path.display());
    println!(
        "selected: {} (duplicates skipped: {}, policy skipped: {}, dry_run: {})",
        plan.cases.len(),
        plan.duplicates_skipped,
        plan.policy_skipped,
        if args.dry_run { "yes" } else { "no" }
    );
    if !plan.policy_skip_reasons.is_empty() {
        println!("policy skip reasons:");
        for reason in &plan.policy_skip_reasons {
            println!(
                "- {}: {} (example task: {})",
                reason.reason_label,
                reason.count,
                clip(&reason.example_task, 72)
            );
        }
    }

    if plan.cases.is_empty() {
        println!("no new replayable seed cases matched the requested filters");
        return Ok(());
    }

    if args.dry_run {
        println!("dry run only; manifest not modified");
        return Ok(());
    }

    append_promoted_cases(&manifest_path, &plan.cases)?;
    println!("appended: {}", plan.cases.len());
    Ok(())
}

fn select_replayable_cases(
    cases: &[BenchmarkCaseSummary],
    category: Option<&str>,
    limit: Option<usize>,
) -> Vec<BenchmarkCaseSummary> {
    let mut selected = cases
        .iter()
        .filter(|case| case.workdir.is_some() && case.seed_observations.is_none())
        .filter(|case| category.is_none_or(|expected| case.category == expected))
        .filter(|case| live_release_replay_eligible(case))
        .cloned()
        .collect::<Vec<_>>();
    if let Some(limit) = limit {
        selected.truncate(limit);
    }
    selected
}

fn live_release_replay_eligible(case: &BenchmarkCaseSummary) -> bool {
    let notes = case.notes.as_deref().unwrap_or("").to_ascii_lowercase();
    !notes.contains("write+validate failure case")
}

#[derive(Debug, Clone)]
struct DogfoodRecord {
    version: u64,
    timestamp_secs: u64,
    duration_ms: u64,
    task: String,
    skill: Option<String>,
    budget: u64,
    model: String,
    model_transport: String,
    workdir: String,
    outcome: DogfoodOutcome,
    manual_intervention: bool,
    notes: Option<String>,
    tool_calls: u64,
    failed_tool_calls: u64,
    repeated_call_failures: u64,
    diagnostic_expected_failure: bool,
    used_subagent: bool,
    final_message: String,
    tool_trace: String,
    error_kind: Option<String>,
    benchmark_category: Option<String>,
    benchmark_seed_observations: Option<String>,
}

impl DogfoodRecord {
    fn from_result(
        timestamp_secs: u64,
        duration_ms: u64,
        model: String,
        model_transport: &'static str,
        workdir: String,
        budget: usize,
        args: &DogfoodRunArgs,
        manual_intervention: bool,
        result: &RunResult,
    ) -> Self {
        let failed_tool_calls = result
            .tool_events
            .iter()
            .filter(|event| tool_event_counts_as_failed(event))
            .count() as u64;
        let repeated_call_failures = result
            .tool_events
            .iter()
            .filter(|event| {
                event
                    .output
                    .contains("repeated identical tool call detected")
            })
            .count() as u64;
        let used_subagent = result
            .tool_events
            .iter()
            .any(|event| event.tool_name == "dispatch_subagent");
        let tool_trace = if result.tool_events.is_empty() {
            "none".to_string()
        } else {
            result
                .tool_events
                .iter()
                .map(|event| event.tool_name.as_str())
                .collect::<Vec<_>>()
                .join(" -> ")
        };
        let diagnostic_expected_failure = args.outcome.is_none()
            && is_expected_failure_diagnosis_result(
                &args.task,
                &result.tool_events,
                repeated_call_failures,
            );
        let recovered_validation_success = args.outcome.is_none()
            && (is_recovered_validation_success(&result.tool_events)
                || is_successful_write_validation_result(&result.tool_events));
        let empty_no_tool_response = args.outcome.is_none()
            && result.tool_events.is_empty()
            && is_empty_model_response(&result.final_message);
        let outcome = args.outcome.unwrap_or_else(|| {
            if empty_no_tool_response {
                DogfoodOutcome::Failed
            } else {
                derive_default_outcome(
                    failed_tool_calls,
                    repeated_call_failures,
                    diagnostic_expected_failure,
                    recovered_validation_success,
                )
            }
        });
        let benchmark_category = infer_benchmark_category(
            &args.task,
            &tool_trace,
            failed_tool_calls,
            repeated_call_failures,
            false,
            None,
        );

        Self {
            version: 1,
            timestamp_secs,
            duration_ms,
            task: args.task.clone(),
            skill: args.skill.clone(),
            budget: budget as u64,
            model,
            model_transport: model_transport.to_string(),
            workdir,
            outcome,
            manual_intervention,
            notes: args.notes.clone(),
            tool_calls: result.tool_events.len() as u64,
            failed_tool_calls,
            repeated_call_failures,
            diagnostic_expected_failure,
            used_subagent,
            final_message: first_non_empty_line(&result.final_message)
                .unwrap_or("")
                .to_string(),
            tool_trace,
            error_kind: None,
            benchmark_category: Some(benchmark_category.to_string()),
            benchmark_seed_observations: serialize_seed_observations(&result.tool_events),
        }
    }

    fn from_error(
        timestamp_secs: u64,
        duration_ms: u64,
        model: String,
        model_transport: &'static str,
        workdir: String,
        budget: usize,
        args: &DogfoodRunArgs,
        manual_intervention: bool,
        error: &(dyn std::error::Error + 'static),
    ) -> Self {
        let outcome = args.outcome.unwrap_or(DogfoodOutcome::Failed);
        let benchmark_category = infer_benchmark_category(&args.task, "none", 1, 0, false, None);
        Self {
            version: 1,
            timestamp_secs,
            duration_ms,
            task: args.task.clone(),
            skill: args.skill.clone(),
            budget: budget as u64,
            model,
            model_transport: model_transport.to_string(),
            workdir,
            outcome,
            manual_intervention,
            notes: args.notes.clone(),
            tool_calls: 0,
            failed_tool_calls: 1,
            repeated_call_failures: 0,
            diagnostic_expected_failure: false,
            used_subagent: false,
            final_message: error.to_string(),
            tool_trace: "none".to_string(),
            error_kind: Some(error_kind_for_ref(error)),
            benchmark_category: Some(benchmark_category.to_string()),
            benchmark_seed_observations: None,
        }
    }

    fn to_json_line(&self) -> String {
        let mut root = BTreeMap::new();
        root.insert(
            "version".to_string(),
            JsonValue::Number(self.version.to_string()),
        );
        root.insert(
            "timestamp_secs".to_string(),
            JsonValue::Number(self.timestamp_secs.to_string()),
        );
        root.insert(
            "duration_ms".to_string(),
            JsonValue::Number(self.duration_ms.to_string()),
        );
        root.insert("task".to_string(), JsonValue::String(self.task.clone()));
        root.insert(
            "skill".to_string(),
            self.skill
                .as_ref()
                .map(|value| JsonValue::String(value.clone()))
                .unwrap_or(JsonValue::Null),
        );
        root.insert(
            "budget".to_string(),
            JsonValue::Number(self.budget.to_string()),
        );
        root.insert("model".to_string(), JsonValue::String(self.model.clone()));
        root.insert(
            "model_transport".to_string(),
            JsonValue::String(self.model_transport.clone()),
        );
        root.insert(
            "workdir".to_string(),
            JsonValue::String(self.workdir.clone()),
        );
        root.insert(
            "outcome".to_string(),
            JsonValue::String(self.outcome.label().to_string()),
        );
        root.insert(
            "manual_intervention".to_string(),
            JsonValue::Bool(self.manual_intervention),
        );
        root.insert(
            "notes".to_string(),
            self.notes
                .as_ref()
                .map(|value| JsonValue::String(value.clone()))
                .unwrap_or(JsonValue::Null),
        );
        root.insert(
            "tool_calls".to_string(),
            JsonValue::Number(self.tool_calls.to_string()),
        );
        root.insert(
            "failed_tool_calls".to_string(),
            JsonValue::Number(self.failed_tool_calls.to_string()),
        );
        root.insert(
            "repeated_call_failures".to_string(),
            JsonValue::Number(self.repeated_call_failures.to_string()),
        );
        root.insert(
            "diagnostic_expected_failure".to_string(),
            JsonValue::Bool(self.diagnostic_expected_failure),
        );
        root.insert(
            "used_subagent".to_string(),
            JsonValue::Bool(self.used_subagent),
        );
        root.insert(
            "final_message".to_string(),
            JsonValue::String(self.final_message.clone()),
        );
        root.insert(
            "tool_trace".to_string(),
            JsonValue::String(self.tool_trace.clone()),
        );
        root.insert(
            "error_kind".to_string(),
            self.error_kind
                .as_ref()
                .map(|value| JsonValue::String(value.clone()))
                .unwrap_or(JsonValue::Null),
        );
        root.insert(
            "benchmark_category".to_string(),
            self.benchmark_category
                .as_ref()
                .map(|value| JsonValue::String(value.clone()))
                .unwrap_or(JsonValue::Null),
        );
        root.insert(
            "benchmark_seed_observations".to_string(),
            self.benchmark_seed_observations
                .as_ref()
                .map(|value| JsonValue::String(value.clone()))
                .unwrap_or(JsonValue::Null),
        );
        json_value_to_string(&JsonValue::Object(root))
    }

    fn from_json_line(line: &str) -> AppResult<Self> {
        let root = parse_root_object(line)?;
        let version = read_u64(&root, "version")?;
        let timestamp_secs = read_u64(&root, "timestamp_secs")?;
        let duration_ms = read_u64(&root, "duration_ms")?;
        let task = read_string(&root, "task")?.to_string();
        let skill = read_optional_string(&root, "skill").map(str::to_string);
        let budget = read_u64(&root, "budget")?;
        let model = read_string(&root, "model")?.to_string();
        let model_transport = read_optional_string(&root, "model_transport")
            .filter(|value| !value.trim().is_empty())
            .unwrap_or(MODEL_TRANSPORT_UNKNOWN)
            .to_string();
        let workdir = read_string(&root, "workdir")?.to_string();
        let outcome = parse_dogfood_outcome(read_string(&root, "outcome")?)
            .ok_or_else(|| app_error("dogfood record has invalid `outcome`"))?;
        let manual_intervention = read_bool(&root, "manual_intervention")?;
        let notes = read_optional_string(&root, "notes").map(str::to_string);
        let tool_calls = read_u64(&root, "tool_calls")?;
        let failed_tool_calls = read_u64(&root, "failed_tool_calls")?;
        let repeated_call_failures = read_u64(&root, "repeated_call_failures")?;
        let diagnostic_expected_failure =
            read_optional_bool(&root, "diagnostic_expected_failure").unwrap_or(false);
        let used_subagent = read_bool(&root, "used_subagent")?;
        let final_message = read_string(&root, "final_message")?.to_string();
        let tool_trace = read_string(&root, "tool_trace")?.to_string();
        let error_kind = read_optional_string(&root, "error_kind").map(str::to_string);
        let stored_benchmark_category =
            read_optional_string(&root, "benchmark_category").map(str::to_string);
        let benchmark_seed_observations =
            read_optional_string(&root, "benchmark_seed_observations").map(str::to_string);

        let inferred_category = infer_benchmark_category(
            &task,
            &tool_trace,
            failed_tool_calls,
            repeated_call_failures,
            used_subagent,
            benchmark_seed_observations.as_deref(),
        )
        .to_string();
        let benchmark_category = match stored_benchmark_category {
            Some(stored)
                if stored == "planning"
                    && inferred_category != "planning"
                    && !task_looks_like_planning(&task) =>
            {
                Some(inferred_category)
            }
            Some(stored) => Some(stored),
            None => Some(inferred_category),
        };

        Ok(Self {
            version,
            timestamp_secs,
            duration_ms,
            task,
            skill,
            budget,
            model,
            model_transport,
            workdir,
            outcome,
            manual_intervention,
            notes,
            tool_calls,
            failed_tool_calls,
            repeated_call_failures,
            diagnostic_expected_failure,
            used_subagent,
            final_message,
            tool_trace,
            error_kind,
            benchmark_category,
            benchmark_seed_observations,
        })
    }
}

impl DogfoodOutcome {
    fn label(&self) -> &'static str {
        match self {
            DogfoodOutcome::Success => "success",
            DogfoodOutcome::Failed => "failed",
            DogfoodOutcome::Stuck => "stuck",
            DogfoodOutcome::Manual => "manual",
        }
    }
}

fn parse_dogfood_outcome(raw: &str) -> Option<DogfoodOutcome> {
    match raw {
        "success" => Some(DogfoodOutcome::Success),
        "failed" => Some(DogfoodOutcome::Failed),
        "stuck" => Some(DogfoodOutcome::Stuck),
        "manual" => Some(DogfoodOutcome::Manual),
        _ => None,
    }
}

fn derive_default_outcome(
    failed_tool_calls: u64,
    repeated_call_failures: u64,
    diagnostic_expected_failure: bool,
    recovered_validation_success: bool,
) -> DogfoodOutcome {
    if diagnostic_expected_failure || recovered_validation_success {
        DogfoodOutcome::Success
    } else if repeated_call_failures > 0 {
        DogfoodOutcome::Stuck
    } else if failed_tool_calls > 0 {
        DogfoodOutcome::Failed
    } else {
        DogfoodOutcome::Success
    }
}

fn is_empty_model_response(message: &str) -> bool {
    message.trim() == "DeepSeek returned no content."
}

fn is_expected_failure_diagnosis_result(
    task: &str,
    events: &[crate::core::loop_runtime::ToolEvent],
    repeated_call_failures: u64,
) -> bool {
    if repeated_call_failures > 0 || !task_looks_like_failure_diagnosis(task) {
        return false;
    }
    let mut saw_expected_failed_command = false;
    let mut saw_follow_up_diagnostic = false;
    for event in events {
        if matches!(event.status, ObservationStatus::Failed) || event.tool_name == "apply_patch" {
            return false;
        }
        if event.tool_name == "run_shell"
            && event
                .output
                .lines()
                .any(|line| line.trim() == "meta.result=failed")
        {
            saw_expected_failed_command = true;
        }
        if matches!(
            event.tool_name.as_str(),
            "read_file" | "search_text" | "list_files"
        ) {
            saw_follow_up_diagnostic = true;
        }
    }
    saw_expected_failed_command && saw_follow_up_diagnostic
}

fn is_recovered_validation_success(events: &[crate::core::loop_runtime::ToolEvent]) -> bool {
    let mut saw_failed_validation = false;
    let mut saw_success_after_failure = false;
    for event in events {
        if matches!(event.status, ObservationStatus::Failed) {
            return false;
        }
        if event.tool_name != "run_shell" {
            continue;
        }
        if event
            .output
            .lines()
            .any(|line| line.trim() == "meta.result=failed")
        {
            saw_failed_validation = true;
            saw_success_after_failure = false;
            continue;
        }
        if event
            .output
            .lines()
            .any(|line| line.trim() == "meta.result=ok")
            && saw_failed_validation
        {
            saw_success_after_failure = true;
        }
    }

    saw_success_after_failure
}

fn is_successful_write_validation_result(events: &[crate::core::loop_runtime::ToolEvent]) -> bool {
    let mut saw_successful_patch = false;
    for event in events {
        if event.tool_name == "apply_patch" && matches!(event.status, ObservationStatus::Ok) {
            saw_successful_patch = true;
            continue;
        }
        if !saw_successful_patch || event.tool_name != "run_shell" {
            continue;
        }
        let is_test = event
            .output
            .lines()
            .any(|line| line.trim() == "meta.command_kind=test");
        let is_ok = event
            .output
            .lines()
            .any(|line| line.trim() == "meta.result=ok");
        if is_test && is_ok {
            return true;
        }
    }
    false
}

fn tool_event_counts_as_failed(event: &crate::core::loop_runtime::ToolEvent) -> bool {
    matches!(event.status, ObservationStatus::Failed)
        || event
            .output
            .lines()
            .any(|line| line.trim() == "meta.result=failed")
}

fn error_kind_label(kind: AppErrorKind) -> String {
    match kind {
        AppErrorKind::Other => "other",
        AppErrorKind::PolicyDenied => "policy_denied",
        AppErrorKind::ToolFailure => "tool_failure",
    }
    .to_string()
}

fn error_kind_for_ref(error: &(dyn std::error::Error + 'static)) -> String {
    error
        .downcast_ref::<AppError>()
        .map(|app| error_kind_label(app.kind))
        .unwrap_or_else(|| error_kind_label(AppErrorKind::Other))
}

fn append_record(path: &Path, record: &DogfoodRecord) -> AppResult<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    writeln!(file, "{}", record.to_json_line())?;
    Ok(())
}

fn load_records(path: &Path) -> AppResult<Vec<DogfoodRecord>> {
    let file = fs::File::open(path).map_err(|error| {
        app_error(format!(
            "failed to read dogfood ledger {}: {error}",
            path.display()
        ))
    })?;
    let reader = BufReader::new(file);
    let mut records = Vec::new();
    for (index, line) in reader.lines().enumerate() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let record = DogfoodRecord::from_json_line(&line).map_err(|error| {
            app_error(format!(
                "failed to parse dogfood ledger line {} in {}: {error}",
                index + 1,
                path.display()
            ))
        })?;
        records.push(record);
    }
    if records.is_empty() {
        return Err(app_error(format!(
            "dogfood ledger {} does not contain any records",
            path.display()
        )));
    }
    Ok(records)
}

fn load_records_or_empty(path: &Path) -> AppResult<Vec<DogfoodRecord>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    load_records(path)
}

#[derive(Debug, Clone)]
struct LivePlan {
    ledger_path: PathBuf,
    manifest_path: PathBuf,
    model_transport: String,
    target_live_runs: usize,
    target_live_success_rate: f64,
    live_runs: usize,
    live_success: usize,
    category_plans: Vec<LiveCategoryPlan>,
}

#[derive(Debug, Clone)]
struct LiveCategoryPlan {
    category: String,
    target_runs: usize,
    target_success_rate: f64,
    live_runs: usize,
    live_success: usize,
    needed_runs: usize,
    replayable_cases: Vec<String>,
    recommended_cases: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct LiveRunCase {
    category: String,
    name: String,
}

fn live_plan_targets(
    configured: Vec<DogfoodCategoryRequirement>,
) -> Vec<DogfoodCategoryRequirement> {
    if !configured.is_empty() {
        return configured;
    }
    LIVE_PLAN_TARGET_CATEGORIES
        .iter()
        .map(
            |(category, min_runs, min_success_percent)| DogfoodCategoryRequirement {
                category: (*category).to_string(),
                min_runs: *min_runs,
                min_success_percent: *min_success_percent,
            },
        )
        .collect()
}

fn build_live_plan(
    ledger_path: &Path,
    manifest_path: &Path,
    records: &[DogfoodRecord],
    summaries: &[BenchmarkCaseSummary],
    model_transport: &str,
    target_live_runs: usize,
    target_live_success_rate: f64,
    targets: &[DogfoodCategoryRequirement],
    limit_per_category: usize,
) -> LivePlan {
    let live_records = records
        .iter()
        .filter(|record| record_is_model_backed(record))
        .collect::<Vec<_>>();
    let live_success = live_records
        .iter()
        .filter(|record| matches!(record.outcome, DogfoodOutcome::Success))
        .count();
    let live_stats = aggregate_category_stats_for(live_records.iter().copied());
    let category_plans = targets
        .iter()
        .map(|target| {
            let stats = live_stats
                .get(&target.category)
                .cloned()
                .unwrap_or_default();
            let replayable_cases = replayable_live_cases_for_category(summaries, &target.category);
            let needed_runs = target.min_runs.saturating_sub(stats.runs);
            let recommended_cases = replayable_cases
                .iter()
                .take(needed_runs.min(limit_per_category))
                .cloned()
                .collect::<Vec<_>>();
            LiveCategoryPlan {
                category: target.category.clone(),
                target_runs: target.min_runs,
                target_success_rate: target.min_success_percent,
                live_runs: stats.runs,
                live_success: stats.success,
                needed_runs,
                replayable_cases,
                recommended_cases,
            }
        })
        .collect::<Vec<_>>();

    LivePlan {
        ledger_path: ledger_path.to_path_buf(),
        manifest_path: manifest_path.to_path_buf(),
        model_transport: model_transport.to_string(),
        target_live_runs,
        target_live_success_rate,
        live_runs: live_records.len(),
        live_success,
        category_plans,
    }
}

fn select_live_run_cases(plan: &LivePlan, categories: &[String], limit: usize) -> Vec<LiveRunCase> {
    let category_filter = categories.iter().cloned().collect::<HashSet<_>>();
    let category_plans = plan
        .category_plans
        .iter()
        .filter(|category| {
            category_filter.is_empty() || category_filter.contains(&category.category)
        })
        .collect::<Vec<_>>();
    let mut selected = Vec::new();
    let mut case_index = 0;
    while selected.len() < limit {
        let mut added_this_pass = false;
        for category in &category_plans {
            if selected.len() >= limit {
                break;
            }
            let Some(case) = category.recommended_cases.get(case_index) else {
                continue;
            };
            selected.push(LiveRunCase {
                category: category.category.clone(),
                name: case.clone(),
            });
            added_this_pass = true;
        }
        if !added_this_pass {
            break;
        }
        case_index += 1;
    }
    selected
}

fn replayable_live_cases_for_category(
    summaries: &[BenchmarkCaseSummary],
    category: &str,
) -> Vec<String> {
    summaries
        .iter()
        .filter(|case| case.category == category)
        .filter(|case| case.workdir.is_some())
        .filter(|case| case.seed_observations.is_none())
        .filter(|case| live_release_replay_eligible(case))
        .map(|case| case.name.clone())
        .collect()
}

fn render_live_plan_text(plan: &LivePlan) -> String {
    let mut out = String::new();
    out.push_str("DeepSeekCode dogfood live plan\n");
    out.push_str(&format!("ledger: {}\n", plan.ledger_path.display()));
    out.push_str(&format!("manifest: {}\n", plan.manifest_path.display()));
    out.push_str(&format!(
        "current_model_transport: {}\n",
        plan.model_transport
    ));
    if plan.model_transport != MODEL_TRANSPORT_ONLINE {
        out.push_str(
            "warning: current model config will not count as model-backed live evidence\n",
        );
    }
    out.push_str(&format!(
        "target_live_runs: {}/{}; success: {} required {:.1}%\n",
        plan.live_runs,
        plan.target_live_runs,
        rate_line(plan.live_success, plan.live_runs),
        plan.target_live_success_rate
    ));
    let overall_needed = plan.target_live_runs.saturating_sub(plan.live_runs);
    out.push_str(&format!("overall_needed_runs: {overall_needed}\n\n"));
    out.push_str(&format!(
        "post_run_report_gate: {}\n\n",
        live_report_gate_command(plan)
    ));
    out.push_str("Category plan:\n");
    for category in &plan.category_plans {
        out.push_str(&format!(
            "- {}: live {}/{}; success {} required {:.1}%; needed {}; replayable_unique {}; recommended_now {}\n",
            category.category,
            category.live_runs,
            category.target_runs,
            rate_line(category.live_success, category.live_runs),
            category.target_success_rate,
            category.needed_runs,
            category.replayable_cases.len(),
            category.recommended_cases.len()
        ));
        if category.replayable_cases.is_empty() && category.needed_runs > 0 {
            out.push_str(
                "  blocker: no replayable workdir-backed benchmark case for this category\n",
            );
        } else if !category.recommended_cases.is_empty() {
            let dry_run_command = live_run_command_line(
                &plan.manifest_path,
                Some(&category.category),
                category.recommended_cases.len(),
                false,
                None,
                None,
            );
            let execute_command = live_run_command_line(
                &plan.manifest_path,
                Some(&category.category),
                category.recommended_cases.len(),
                true,
                None,
                None,
            );
            out.push_str(&format!("  dry_run: {dry_run_command}\n"));
            out.push_str(&format!("  execute: {execute_command}\n"));
            out.push_str(&format!(
                "  cases: {}\n",
                category.recommended_cases.join(", ")
            ));
            if category.needed_runs > category.replayable_cases.len() {
                out.push_str(&format!(
                    "  note: {} more run(s) remain after one unique replay pass; add more fixtures or repeat carefully after reviewing outcomes\n",
                    category.needed_runs - category.replayable_cases.len()
                ));
            }
        }
    }
    out
}

fn render_live_plan_json(plan: &LivePlan) -> String {
    let categories = plan
        .category_plans
        .iter()
        .map(|category| {
            let mut root = BTreeMap::new();
            root.insert(
                "category".to_string(),
                JsonValue::String(category.category.clone()),
            );
            root.insert(
                "target_runs".to_string(),
                JsonValue::Number(category.target_runs.to_string()),
            );
            root.insert(
                "target_success_rate".to_string(),
                JsonValue::Number(format!("{:.1}", category.target_success_rate)),
            );
            root.insert(
                "live_runs".to_string(),
                JsonValue::Number(category.live_runs.to_string()),
            );
            root.insert(
                "live_success".to_string(),
                JsonValue::Number(category.live_success.to_string()),
            );
            root.insert(
                "needed_runs".to_string(),
                JsonValue::Number(category.needed_runs.to_string()),
            );
            root.insert(
                "replayable_cases".to_string(),
                JsonValue::Array(
                    category
                        .replayable_cases
                        .iter()
                        .cloned()
                        .map(JsonValue::String)
                        .collect(),
                ),
            );
            root.insert(
                "recommended_cases".to_string(),
                JsonValue::Array(
                    category
                        .recommended_cases
                        .iter()
                        .cloned()
                        .map(JsonValue::String)
                        .collect(),
                ),
            );
            if !category.recommended_cases.is_empty() {
                root.insert(
                    "live_run_command".to_string(),
                    JsonValue::String(live_run_command_line(
                        &plan.manifest_path,
                        Some(&category.category),
                        category.recommended_cases.len(),
                        false,
                        None,
                        None,
                    )),
                );
                root.insert(
                    "live_run_execute_command".to_string(),
                    JsonValue::String(live_run_command_line(
                        &plan.manifest_path,
                        Some(&category.category),
                        category.recommended_cases.len(),
                        true,
                        None,
                        None,
                    )),
                );
            }
            JsonValue::Object(root)
        })
        .collect::<Vec<_>>();

    let mut root = BTreeMap::new();
    root.insert(
        "ledger".to_string(),
        JsonValue::String(plan.ledger_path.display().to_string()),
    );
    root.insert(
        "manifest".to_string(),
        JsonValue::String(plan.manifest_path.display().to_string()),
    );
    root.insert(
        "model_transport".to_string(),
        JsonValue::String(plan.model_transport.clone()),
    );
    root.insert(
        "target_live_runs".to_string(),
        JsonValue::Number(plan.target_live_runs.to_string()),
    );
    root.insert(
        "target_live_success_rate".to_string(),
        JsonValue::Number(format!("{:.1}", plan.target_live_success_rate)),
    );
    root.insert(
        "live_runs".to_string(),
        JsonValue::Number(plan.live_runs.to_string()),
    );
    root.insert(
        "live_success".to_string(),
        JsonValue::Number(plan.live_success.to_string()),
    );
    root.insert(
        "overall_needed_runs".to_string(),
        JsonValue::Number(
            plan.target_live_runs
                .saturating_sub(plan.live_runs)
                .to_string(),
        ),
    );
    root.insert(
        "post_run_report_command".to_string(),
        JsonValue::String(live_report_gate_command(plan)),
    );
    root.insert("evidence_gate".to_string(), live_report_gate_json(plan));
    root.insert("categories".to_string(), JsonValue::Array(categories));
    json_value_to_string(&JsonValue::Object(root))
}

fn live_run_command_line(
    manifest_path: &Path,
    category: Option<&str>,
    limit: usize,
    execute: bool,
    api_key_file: Option<&str>,
    evidence_out: Option<&str>,
) -> String {
    let categories = category
        .map(|value| vec![value.to_string()])
        .unwrap_or_default();
    live_run_command_line_for_categories(
        manifest_path,
        &categories,
        limit,
        execute,
        false,
        api_key_file,
        evidence_out,
    )
}

fn live_run_command_line_for_categories(
    manifest_path: &Path,
    categories: &[String],
    limit: usize,
    execute: bool,
    json: bool,
    api_key_file: Option<&str>,
    evidence_out: Option<&str>,
) -> String {
    let mut command = format!(
        "deepseek dogfood live-run --manifest {}",
        shell_quote(&manifest_path.display().to_string())
    );
    if let Some(api_key_file) = api_key_file {
        command.push_str(" --api-key-file ");
        command.push_str(&shell_quote(api_key_file));
    }
    if let Some(evidence_out) = evidence_out {
        command.push_str(" --evidence-out ");
        command.push_str(&shell_quote(evidence_out));
    }
    for category in categories {
        command.push_str(" --category ");
        command.push_str(&shell_quote(category));
    }
    command.push_str(" --limit ");
    command.push_str(&limit.to_string());
    if json {
        command.push_str(" --json");
    }
    if execute {
        command.push_str(" --execute");
    }
    command
}

fn render_live_run_plan_json(
    plan: &LivePlan,
    requested_categories: &[String],
    limit: usize,
    selected: &[LiveRunCase],
    api_key_file: Option<&str>,
    evidence_out: Option<&str>,
) -> String {
    let selected_cases = selected
        .iter()
        .map(|case| {
            let mut root = BTreeMap::new();
            root.insert(
                "category".to_string(),
                JsonValue::String(case.category.clone()),
            );
            root.insert("name".to_string(), JsonValue::String(case.name.clone()));
            JsonValue::Object(root)
        })
        .collect::<Vec<_>>();
    let online_ready = plan.model_transport == MODEL_TRANSPORT_ONLINE;
    let execute_blocker = if selected.is_empty() {
        Some("no recommended live dogfood cases matched the requested filters")
    } else if !online_ready {
        Some("dogfood live-run --execute requires an online model transport; configure the provider API key first")
    } else {
        None
    };

    let mut root = BTreeMap::new();
    root.insert(
        "kind".to_string(),
        JsonValue::String("deepseek.dogfood.live_run_plan.v1".to_string()),
    );
    root.insert(
        "ledger".to_string(),
        JsonValue::String(plan.ledger_path.display().to_string()),
    );
    root.insert(
        "manifest".to_string(),
        JsonValue::String(plan.manifest_path.display().to_string()),
    );
    root.insert(
        "model_transport".to_string(),
        JsonValue::String(plan.model_transport.clone()),
    );
    root.insert("online_ready".to_string(), JsonValue::Bool(online_ready));
    root.insert(
        "credential_source".to_string(),
        JsonValue::String(if api_key_file.is_some() {
            "api_key_file".to_string()
        } else if online_ready {
            "env".to_string()
        } else {
            "missing".to_string()
        }),
    );
    root.insert(
        "api_key_file".to_string(),
        api_key_file
            .map(|path| JsonValue::String(path.to_string()))
            .unwrap_or(JsonValue::Null),
    );
    root.insert(
        "evidence_out".to_string(),
        evidence_out
            .map(|path| JsonValue::String(path.to_string()))
            .unwrap_or(JsonValue::Null),
    );
    root.insert(
        "execute_ready".to_string(),
        JsonValue::Bool(execute_blocker.is_none()),
    );
    root.insert(
        "execute_blocker".to_string(),
        execute_blocker
            .map(|value| JsonValue::String(value.to_string()))
            .unwrap_or(JsonValue::Null),
    );
    root.insert(
        "target_live_runs".to_string(),
        JsonValue::Number(plan.target_live_runs.to_string()),
    );
    root.insert(
        "target_live_success_rate".to_string(),
        JsonValue::Number(format!("{:.1}", plan.target_live_success_rate)),
    );
    root.insert(
        "live_runs".to_string(),
        JsonValue::Number(plan.live_runs.to_string()),
    );
    root.insert(
        "live_success".to_string(),
        JsonValue::Number(plan.live_success.to_string()),
    );
    root.insert(
        "live_success_rate".to_string(),
        JsonValue::Number(format!(
            "{:.1}",
            rate_percent(plan.live_success, plan.live_runs)
        )),
    );
    root.insert("limit".to_string(), JsonValue::Number(limit.to_string()));
    root.insert(
        "requested_categories".to_string(),
        JsonValue::Array(
            requested_categories
                .iter()
                .cloned()
                .map(JsonValue::String)
                .collect(),
        ),
    );
    root.insert(
        "selected_count".to_string(),
        JsonValue::Number(selected.len().to_string()),
    );
    root.insert(
        "selected_cases".to_string(),
        JsonValue::Array(selected_cases),
    );
    root.insert(
        "dry_run_command".to_string(),
        JsonValue::String(live_run_command_line_for_categories(
            &plan.manifest_path,
            requested_categories,
            limit,
            false,
            true,
            api_key_file,
            evidence_out,
        )),
    );
    root.insert(
        "execute_command".to_string(),
        JsonValue::String(live_run_command_line_for_categories(
            &plan.manifest_path,
            requested_categories,
            limit,
            true,
            false,
            api_key_file,
            evidence_out,
        )),
    );
    root.insert(
        "post_run_report_command".to_string(),
        JsonValue::String(live_report_gate_command(plan)),
    );
    root.insert("evidence_gate".to_string(), live_report_gate_json(plan));
    json_value_to_string(&JsonValue::Object(root))
}

fn live_run_case_evidence_json(
    case: &LiveRunCase,
    appended_records: &[DogfoodRecord],
    error: Option<&str>,
) -> JsonValue {
    let mut root = BTreeMap::new();
    root.insert(
        "category".to_string(),
        JsonValue::String(case.category.clone()),
    );
    root.insert("name".to_string(), JsonValue::String(case.name.clone()));
    root.insert(
        "ledger_records_appended".to_string(),
        JsonValue::Number(appended_records.len().to_string()),
    );
    root.insert(
        "model_backed_records_appended".to_string(),
        JsonValue::Number(
            appended_records
                .iter()
                .filter(|record| record_is_model_backed(record))
                .count()
                .to_string(),
        ),
    );
    root.insert(
        "error".to_string(),
        error
            .map(|value| JsonValue::String(value.to_string()))
            .unwrap_or(JsonValue::Null),
    );

    if let Some(record) = appended_records.last() {
        root.insert(
            "timestamp_secs".to_string(),
            JsonValue::Number(record.timestamp_secs.to_string()),
        );
        root.insert(
            "duration_ms".to_string(),
            JsonValue::Number(record.duration_ms.to_string()),
        );
        root.insert(
            "outcome".to_string(),
            JsonValue::String(record.outcome.label().to_string()),
        );
        root.insert(
            "model_transport".to_string(),
            JsonValue::String(record.model_transport.clone()),
        );
        root.insert(
            "model_backed".to_string(),
            JsonValue::Bool(record_is_model_backed(record)),
        );
        root.insert(
            "manual_intervention".to_string(),
            JsonValue::Bool(record.manual_intervention),
        );
        root.insert(
            "benchmark_category".to_string(),
            record
                .benchmark_category
                .as_ref()
                .map(|value| JsonValue::String(value.clone()))
                .unwrap_or(JsonValue::Null),
        );
        root.insert(
            "error_kind".to_string(),
            record
                .error_kind
                .as_ref()
                .map(|value| JsonValue::String(value.clone()))
                .unwrap_or(JsonValue::Null),
        );
    } else {
        root.insert("timestamp_secs".to_string(), JsonValue::Null);
        root.insert("duration_ms".to_string(), JsonValue::Null);
        root.insert("outcome".to_string(), JsonValue::Null);
        root.insert("model_transport".to_string(), JsonValue::Null);
        root.insert("model_backed".to_string(), JsonValue::Bool(false));
        root.insert("manual_intervention".to_string(), JsonValue::Bool(false));
        root.insert("benchmark_category".to_string(), JsonValue::Null);
        root.insert("error_kind".to_string(), JsonValue::Null);
    }

    JsonValue::Object(root)
}

#[allow(clippy::too_many_arguments)]
fn live_run_evidence_summary_json(
    plan: &LivePlan,
    requested_categories: &[String],
    limit: usize,
    selected: &[LiveRunCase],
    case_evidence: &[JsonValue],
    before_records: &[DogfoodRecord],
    after_records: &[DogfoodRecord],
    api_key_file: Option<&str>,
    evidence_out: Option<&str>,
    benchmark_gate_requested: bool,
    ledger_fingerprint: JsonValue,
    run_error: Option<String>,
    benchmark_gate_error: Option<String>,
) -> JsonValue {
    let appended_records = after_records.get(before_records.len()..).unwrap_or(&[]);
    let online_ready = plan.model_transport == MODEL_TRANSPORT_ONLINE;
    let completed = run_error.is_none() && benchmark_gate_error.is_none();

    let mut benchmark_gate = BTreeMap::new();
    benchmark_gate.insert(
        "requested".to_string(),
        JsonValue::Bool(benchmark_gate_requested),
    );
    benchmark_gate.insert(
        "passed".to_string(),
        JsonValue::Bool(
            benchmark_gate_requested && benchmark_gate_error.is_none() && run_error.is_none(),
        ),
    );
    benchmark_gate.insert(
        "error".to_string(),
        benchmark_gate_error
            .map(JsonValue::String)
            .unwrap_or(JsonValue::Null),
    );

    let mut root = BTreeMap::new();
    root.insert(
        "kind".to_string(),
        JsonValue::String("deepseek.dogfood.live_run_evidence.v1".to_string()),
    );
    root.insert(
        "ledger".to_string(),
        JsonValue::String(plan.ledger_path.display().to_string()),
    );
    root.insert(
        "manifest".to_string(),
        JsonValue::String(plan.manifest_path.display().to_string()),
    );
    root.insert(
        "model_transport".to_string(),
        JsonValue::String(plan.model_transport.clone()),
    );
    root.insert("online_ready".to_string(), JsonValue::Bool(online_ready));
    root.insert(
        "credential_source".to_string(),
        JsonValue::String(if api_key_file.is_some() {
            "api_key_file".to_string()
        } else if online_ready {
            "env".to_string()
        } else {
            "missing".to_string()
        }),
    );
    root.insert(
        "api_key_file".to_string(),
        api_key_file
            .map(|path| JsonValue::String(path.to_string()))
            .unwrap_or(JsonValue::Null),
    );
    root.insert(
        "evidence_out".to_string(),
        evidence_out
            .map(|path| JsonValue::String(path.to_string()))
            .unwrap_or(JsonValue::Null),
    );
    root.insert("completed".to_string(), JsonValue::Bool(completed));
    root.insert(
        "error".to_string(),
        run_error.map(JsonValue::String).unwrap_or(JsonValue::Null),
    );
    root.insert("limit".to_string(), JsonValue::Number(limit.to_string()));
    root.insert(
        "requested_categories".to_string(),
        JsonValue::Array(
            requested_categories
                .iter()
                .cloned()
                .map(JsonValue::String)
                .collect(),
        ),
    );
    root.insert(
        "selected_count".to_string(),
        JsonValue::Number(selected.len().to_string()),
    );
    root.insert(
        "cases".to_string(),
        JsonValue::Array(case_evidence.to_vec()),
    );
    root.insert(
        "before".to_string(),
        live_run_records_snapshot_json(before_records),
    );
    root.insert(
        "after".to_string(),
        live_run_records_snapshot_json(after_records),
    );
    root.insert(
        "appended_records".to_string(),
        JsonValue::Number(appended_records.len().to_string()),
    );
    root.insert(
        "appended_model_backed_records".to_string(),
        JsonValue::Number(
            appended_records
                .iter()
                .filter(|record| record_is_model_backed(record))
                .count()
                .to_string(),
        ),
    );
    root.insert(
        "benchmark_gate".to_string(),
        JsonValue::Object(benchmark_gate),
    );
    root.insert("ledger_fingerprint".to_string(), ledger_fingerprint);
    root.insert(
        "post_run_report_command".to_string(),
        JsonValue::String(live_report_gate_command(plan)),
    );
    root.insert("evidence_gate".to_string(), live_report_gate_json(plan));
    JsonValue::Object(root)
}

#[allow(clippy::too_many_arguments)]
fn external_fixture_evidence_summary_json(
    source_workdir: &Path,
    ledger_path: &Path,
    report_path: &Path,
    args: &DogfoodExternalFixtureArgs,
    model_transport: &str,
    before_records: &[DogfoodRecord],
    after_records: &[DogfoodRecord],
    ledger_fingerprint: JsonValue,
    run_error: Option<String>,
    post_validation_command: &str,
) -> JsonValue {
    let appended_records = after_records.get(before_records.len()..).unwrap_or(&[]);
    let appended_external_write_fixtures = appended_records
        .iter()
        .filter(|record| is_external_write_fixture_record(record))
        .count();
    let appended_successful_external_write_fixtures = appended_records
        .iter()
        .filter(|record| {
            is_external_write_fixture_record(record)
                && record_is_model_backed(record)
                && (matches!(record.outcome, DogfoodOutcome::Success) || run_error.is_none())
        })
        .count();
    let appended_model_backed_records = appended_records
        .iter()
        .filter(|record| record_is_model_backed(record))
        .count();
    let online_ready = model_transport == MODEL_TRANSPORT_ONLINE;
    let completed = run_error.is_none();
    let release_evidence_ready =
        completed && online_ready && appended_successful_external_write_fixtures > 0;

    let mut root = BTreeMap::new();
    root.insert(
        "kind".to_string(),
        JsonValue::String("deepseek.dogfood.external_fixture_evidence.v1".to_string()),
    );
    root.insert(
        "source_workdir".to_string(),
        JsonValue::String(source_workdir.display().to_string()),
    );
    root.insert(
        "ledger".to_string(),
        JsonValue::String(ledger_path.display().to_string()),
    );
    root.insert(
        "report".to_string(),
        JsonValue::String(report_path.display().to_string()),
    );
    root.insert("task".to_string(), JsonValue::String(args.task.clone()));
    root.insert(
        "budget".to_string(),
        args.budget
            .map(|budget| JsonValue::Number(budget.to_string()))
            .unwrap_or(JsonValue::Null),
    );
    root.insert(
        "notes".to_string(),
        args.notes
            .clone()
            .map(JsonValue::String)
            .unwrap_or(JsonValue::Null),
    );
    root.insert(
        "benchmark_gate_requested".to_string(),
        JsonValue::Bool(args.benchmark_gate),
    );
    root.insert(
        "post_validation_command".to_string(),
        JsonValue::String(post_validation_command.to_string()),
    );
    root.insert(
        "post_validation_passed".to_string(),
        JsonValue::Bool(completed),
    );
    root.insert(
        "model_transport".to_string(),
        JsonValue::String(model_transport.to_string()),
    );
    root.insert("online_ready".to_string(), JsonValue::Bool(online_ready));
    root.insert(
        "allow_offline".to_string(),
        JsonValue::Bool(args.allow_offline),
    );
    root.insert(
        "rehearsal".to_string(),
        JsonValue::Bool(args.allow_offline || !online_ready),
    );
    root.insert("completed".to_string(), JsonValue::Bool(completed));
    root.insert(
        "error".to_string(),
        run_error.map(JsonValue::String).unwrap_or(JsonValue::Null),
    );
    root.insert(
        "release_evidence_ready".to_string(),
        JsonValue::Bool(release_evidence_ready),
    );
    root.insert(
        "before".to_string(),
        live_run_records_snapshot_json(before_records),
    );
    root.insert(
        "after".to_string(),
        live_run_records_snapshot_json(after_records),
    );
    root.insert(
        "appended_records".to_string(),
        JsonValue::Number(appended_records.len().to_string()),
    );
    root.insert(
        "appended_model_backed_records".to_string(),
        JsonValue::Number(appended_model_backed_records.to_string()),
    );
    root.insert(
        "appended_external_write_fixtures".to_string(),
        JsonValue::Number(appended_external_write_fixtures.to_string()),
    );
    root.insert(
        "appended_successful_external_write_fixtures".to_string(),
        JsonValue::Number(appended_successful_external_write_fixtures.to_string()),
    );
    root.insert(
        "records".to_string(),
        JsonValue::Array(
            appended_records
                .iter()
                .map(dogfood_record_json_value)
                .collect(),
        ),
    );
    root.insert("ledger_fingerprint".to_string(), ledger_fingerprint);
    JsonValue::Object(root)
}

fn dogfood_record_json_value(record: &DogfoodRecord) -> JsonValue {
    match parse_root_object(&record.to_json_line()) {
        Ok(mut root) => {
            root.insert(
                "model_backed".to_string(),
                JsonValue::Bool(record_is_model_backed(record)),
            );
            JsonValue::Object(root)
        }
        Err(_) => JsonValue::String(record.to_json_line()),
    }
}

fn dogfood_file_fingerprint_json(path: &Path) -> JsonValue {
    let mut root = BTreeMap::new();
    root.insert("ok".to_string(), JsonValue::Bool(false));
    root.insert(
        "path".to_string(),
        JsonValue::String(path.display().to_string()),
    );
    root.insert(
        "algorithm".to_string(),
        JsonValue::String("fnv1a64".to_string()),
    );
    match fs::read(path) {
        Ok(bytes) => {
            root.insert("ok".to_string(), JsonValue::Bool(true));
            root.insert(
                "bytes".to_string(),
                JsonValue::Number(bytes.len().to_string()),
            );
            root.insert(
                "fnv1a64".to_string(),
                JsonValue::String(fnv1a64_hex(&bytes)),
            );
            root.insert("error".to_string(), JsonValue::Null);
        }
        Err(error) => {
            root.insert("bytes".to_string(), JsonValue::Null);
            root.insert("fnv1a64".to_string(), JsonValue::Null);
            root.insert("error".to_string(), JsonValue::String(error.to_string()));
        }
    }
    JsonValue::Object(root)
}

fn fnv1a64_hex(bytes: &[u8]) -> String {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}

fn live_run_records_snapshot_json(records: &[DogfoodRecord]) -> JsonValue {
    let live_runs = records
        .iter()
        .filter(|record| record_is_model_backed(record))
        .count();
    let live_success = records
        .iter()
        .filter(|record| {
            record_is_model_backed(record) && matches!(record.outcome, DogfoodOutcome::Success)
        })
        .count();

    let mut root = BTreeMap::new();
    root.insert(
        "total_records".to_string(),
        JsonValue::Number(records.len().to_string()),
    );
    root.insert(
        "live_runs".to_string(),
        JsonValue::Number(live_runs.to_string()),
    );
    root.insert(
        "live_success".to_string(),
        JsonValue::Number(live_success.to_string()),
    );
    root.insert(
        "live_success_rate".to_string(),
        JsonValue::Number(format!("{:.1}", rate_percent(live_success, live_runs))),
    );
    JsonValue::Object(root)
}

fn write_live_run_evidence_summary(path: &str, summary: &JsonValue) -> AppResult<()> {
    write_dogfood_json_artifact(path, summary, "dogfood live-run evidence summary")
}

fn write_external_fixture_evidence_summary(path: &str, summary: &JsonValue) -> AppResult<()> {
    write_dogfood_json_artifact(path, summary, "dogfood external-fixture evidence summary")
}

fn write_dogfood_json_artifact(path: &str, value: &JsonValue, label: &str) -> AppResult<()> {
    let path = PathBuf::from(path);
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)?;
        }
    }
    fs::write(&path, format!("{}\n", json_value_to_string(value))).map_err(|error| {
        app_error(format!(
            "failed to write {label} {}: {error}",
            path.display()
        ))
    })
}

fn live_report_gate_command(plan: &LivePlan) -> String {
    let report_limit = plan.target_live_runs.clamp(DEFAULT_REPORT_LIMIT, 500);
    let mut command = format!(
        "deepseek dogfood report --limit {} --require-live-runs {} --require-live-success-rate {}",
        report_limit,
        plan.target_live_runs,
        format_percent_command_arg(plan.target_live_success_rate)
    );
    for category in &plan.category_plans {
        command.push_str(" --require-live-category ");
        command.push_str(&shell_quote(&format!(
            "{}:{}:{}",
            category.category,
            category.target_runs,
            format_percent_command_arg(category.target_success_rate)
        )));
    }
    command
}

fn live_report_gate_json(plan: &LivePlan) -> JsonValue {
    let categories = plan
        .category_plans
        .iter()
        .map(|category| {
            let mut root = BTreeMap::new();
            root.insert(
                "category".to_string(),
                JsonValue::String(category.category.clone()),
            );
            root.insert(
                "min_runs".to_string(),
                JsonValue::Number(category.target_runs.to_string()),
            );
            root.insert(
                "min_success_rate".to_string(),
                JsonValue::Number(format!("{:.1}", category.target_success_rate)),
            );
            JsonValue::Object(root)
        })
        .collect::<Vec<_>>();

    let mut root = BTreeMap::new();
    root.insert(
        "require_live_runs".to_string(),
        JsonValue::Number(plan.target_live_runs.to_string()),
    );
    root.insert(
        "require_live_success_rate".to_string(),
        JsonValue::Number(format!("{:.1}", plan.target_live_success_rate)),
    );
    root.insert(
        "require_live_categories".to_string(),
        JsonValue::Array(categories),
    );
    root.insert(
        "command".to_string(),
        JsonValue::String(live_report_gate_command(plan)),
    );
    JsonValue::Object(root)
}

fn format_percent_command_arg(value: f64) -> String {
    if (value.fract()).abs() < f64::EPSILON {
        format!("{value:.0}")
    } else {
        format!("{value:.1}")
    }
}

fn shell_quote(value: &str) -> String {
    if value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '/' | '.' | '_' | '-' | ':'))
    {
        value.to_string()
    } else {
        format!("'{}'", value.replace('\'', "'\\''"))
    }
}

fn write_report(
    ledger_path: &Path,
    report_path: &Path,
    records: &[DogfoodRecord],
    limit: usize,
) -> AppResult<()> {
    if let Some(parent) = report_path.parent() {
        fs::create_dir_all(parent)?;
    }
    let report = render_report(ledger_path, records, limit);
    fs::write(report_path, report)?;
    Ok(())
}

fn render_report(ledger_path: &Path, records: &[DogfoodRecord], limit: usize) -> String {
    let total = records.len();
    let success = records
        .iter()
        .filter(|record| matches!(record.outcome, DogfoodOutcome::Success))
        .count();
    let model_backed = records
        .iter()
        .filter(|record| record_is_model_backed(record))
        .count();
    let successful_model_backed = records
        .iter()
        .filter(|record| {
            record_is_model_backed(record) && matches!(record.outcome, DogfoodOutcome::Success)
        })
        .count();
    let diagnostic = records
        .iter()
        .filter(|record| record.diagnostic_expected_failure)
        .count();
    let stuck = records
        .iter()
        .filter(|record| matches!(record.outcome, DogfoodOutcome::Stuck))
        .count();
    let manual = records
        .iter()
        .filter(|record| record.manual_intervention)
        .count();
    let failed = records
        .iter()
        .filter(|record| matches!(record.outcome, DogfoodOutcome::Failed))
        .count();
    let total_tool_calls = records.iter().map(|record| record.tool_calls).sum::<u64>();
    let overall_avg_tool_calls = if total == 0 {
        0.0
    } else {
        total_tool_calls as f64 / total as f64
    };
    let benchmark_seed_candidates = records
        .iter()
        .filter(|record| is_benchmark_seed_candidate(record, None))
        .count();
    let external_write_fixtures = records
        .iter()
        .filter(|record| is_external_write_fixture_record(record))
        .count();
    let successful_external_write_fixtures = records
        .iter()
        .filter(|record| {
            is_external_write_fixture_record(record)
                && matches!(record.outcome, DogfoodOutcome::Success)
        })
        .count();
    let category_stats = aggregate_category_stats(records);
    let recent_start = total.saturating_sub(CATEGORY_TREND_WINDOW);
    let previous_start = recent_start.saturating_sub(CATEGORY_TREND_WINDOW);
    let recent_window = &records[recent_start..];
    let previous_window = &records[previous_start..recent_start];
    let recent_category_stats = aggregate_category_stats(recent_window);
    let previous_category_stats = aggregate_category_stats(previous_window);

    let mut out = String::new();
    out.push_str("# DeepSeekCode Dogfood Report\n\n");
    out.push_str(&format!("- Ledger: `{}`\n", ledger_path.display()));
    out.push_str(&format!("- Runs: {total}\n"));
    out.push_str(&format!("- Success rate: {}\n", rate_line(success, total)));
    out.push_str(&format!(
        "- Diagnostic expected-failure rate: {}\n",
        rate_line(diagnostic, total)
    ));
    out.push_str(&format!("- Failed rate: {}\n", rate_line(failed, total)));
    out.push_str(&format!("- Stuck rate: {}\n", rate_line(stuck, total)));
    out.push_str(&format!(
        "- Manual intervention rate: {}\n",
        rate_line(manual, total)
    ));
    out.push_str(&format!(
        "- Average tool calls: {:.2}\n\n",
        overall_avg_tool_calls
    ));
    out.push_str(&format!(
        "- Model-backed runs: {}\n",
        rate_line(successful_model_backed, model_backed)
    ));
    out.push_str(&format!(
        "- Benchmark seed candidates: {benchmark_seed_candidates}\n"
    ));
    out.push_str(&format!(
        "- External write fixtures: {}\n\n",
        rate_line(successful_external_write_fixtures, external_write_fixtures)
    ));
    if !category_stats.is_empty() {
        out.push_str("## Category Breakdown\n\n");
        out.push_str(
            "| Category | Runs | Success | Diagnostic | Failed | Stuck | Manual | Avg Tool Calls | Seed Candidates |\n",
        );
        out.push_str("| --- | ---: | --- | --- | --- | --- | --- | ---: | ---: |\n");
        for (category, stats) in &category_stats {
            let avg_tool_calls = if stats.runs == 0 {
                0.0
            } else {
                stats.total_tool_calls as f64 / stats.runs as f64
            };
            out.push_str(&format!(
                "| {} | {} | {} | {} | {} | {} | {} | {:.2} | {} |\n",
                escape_table(category),
                stats.runs,
                rate_line(stats.success, stats.runs),
                rate_line(stats.diagnostic, stats.runs),
                rate_line(stats.failed, stats.runs),
                rate_line(stats.stuck, stats.runs),
                rate_line(stats.manual, stats.runs),
                avg_tool_calls,
                stats.seed_candidates,
            ));
        }
        out.push('\n');
    }
    out.push_str("## Category Trend\n\n");
    out.push_str(&format!(
        "- Trend window: recent {} runs vs previous {} runs\n",
        CATEGORY_TREND_WINDOW, CATEGORY_TREND_WINDOW
    ));
    if previous_window.is_empty() {
        out.push_str("- Status: insufficient history\n\n");
    } else {
        out.push_str(&format!(
            "- Compared windows: recent={} previous={}\n\n",
            recent_window.len(),
            previous_window.len()
        ));
        out.push_str(
            "| Category | Recent Runs | Prev Runs | Recent Success | Prev Success | Δ Success pp | Recent Avg Tools | Prev Avg Tools | Δ Tools | Recent Seeds | Prev Seeds |\n",
        );
        out.push_str(
            "| --- | ---: | ---: | --- | --- | ---: | ---: | ---: | ---: | ---: | ---: |\n",
        );
        let mut categories = BTreeMap::<String, ()>::new();
        for category in recent_category_stats.keys() {
            categories.insert(category.clone(), ());
        }
        for category in previous_category_stats.keys() {
            categories.insert(category.clone(), ());
        }
        for category in categories.keys() {
            let recent = recent_category_stats
                .get(category)
                .cloned()
                .unwrap_or_default();
            let previous = previous_category_stats
                .get(category)
                .cloned()
                .unwrap_or_default();
            out.push_str(&format!(
                "| {} | {} | {} | {} | {} | {:+.1} | {:.2} | {:.2} | {:+.2} | {} | {} |\n",
                escape_table(category),
                recent.runs,
                previous.runs,
                rate_line(recent.success, recent.runs),
                rate_line(previous.success, previous.runs),
                rate_percent(recent.success, recent.runs)
                    - rate_percent(previous.success, previous.runs),
                avg_tool_calls(&recent),
                avg_tool_calls(&previous),
                avg_tool_calls(&recent) - avg_tool_calls(&previous),
                recent.seed_candidates,
                previous.seed_candidates,
            ));
        }
        out.push('\n');
    }
    out.push_str("| Timestamp | Category | Outcome | Transport | Manual | Budget | Tool Calls | Failed Tools | Workdir | Task | Notes |\n");
    out.push_str("| --- | --- | --- | --- | --- | ---: | ---: | ---: | --- | --- | --- |\n");
    for record in records.iter().rev().take(limit) {
        out.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} |\n",
            record.timestamp_secs,
            escape_table(&benchmark_case_category(record)),
            report_outcome_label(record),
            escape_table(&record.model_transport),
            if record.manual_intervention {
                "yes"
            } else {
                "no"
            },
            record.budget,
            record.tool_calls,
            record.failed_tool_calls,
            escape_table(&clip(&record.workdir, 36)),
            escape_table(&clip(&record.task, 56)),
            escape_table(&clip(record.notes.as_deref().unwrap_or(""), 48)),
        ));
    }
    out
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct DogfoodCategoryStats {
    runs: usize,
    success: usize,
    diagnostic: usize,
    failed: usize,
    stuck: usize,
    manual: usize,
    total_tool_calls: u64,
    seed_candidates: usize,
}

fn aggregate_category_stats(records: &[DogfoodRecord]) -> BTreeMap<String, DogfoodCategoryStats> {
    aggregate_category_stats_for(records.iter())
}

fn aggregate_category_stats_for<'a, I>(records: I) -> BTreeMap<String, DogfoodCategoryStats>
where
    I: IntoIterator<Item = &'a DogfoodRecord>,
{
    let mut category_stats = BTreeMap::<String, DogfoodCategoryStats>::new();
    for record in records {
        let category = benchmark_case_category(record).to_string();
        let stats = category_stats.entry(category).or_default();
        stats.runs += 1;
        stats.total_tool_calls += record.tool_calls;
        if record.manual_intervention {
            stats.manual += 1;
        }
        if is_benchmark_seed_candidate(record, None) {
            stats.seed_candidates += 1;
        }
        if record.diagnostic_expected_failure {
            stats.diagnostic += 1;
        }
        match record.outcome {
            DogfoodOutcome::Success => stats.success += 1,
            DogfoodOutcome::Failed => stats.failed += 1,
            DogfoodOutcome::Stuck => stats.stuck += 1,
            DogfoodOutcome::Manual => {}
        }
    }
    category_stats
}

fn report_outcome_label(record: &DogfoodRecord) -> &'static str {
    if record.diagnostic_expected_failure {
        "diagnostic"
    } else {
        record.outcome.label()
    }
}

fn avg_tool_calls(stats: &DogfoodCategoryStats) -> f64 {
    if stats.runs == 0 {
        0.0
    } else {
        stats.total_tool_calls as f64 / stats.runs as f64
    }
}

fn rate_percent(count: usize, total: usize) -> f64 {
    if total == 0 {
        0.0
    } else {
        (count as f64 / total as f64) * 100.0
    }
}

fn rate_line(count: usize, total: usize) -> String {
    if total == 0 {
        return "0/0 (0.0%)".to_string();
    }
    format!(
        "{count}/{total} ({:.1}%)",
        (count as f64 / total as f64) * 100.0
    )
}

fn serialize_seed_observations(events: &[crate::core::loop_runtime::ToolEvent]) -> Option<String> {
    if events.is_empty() {
        return None;
    }
    let entries = events
        .iter()
        .rev()
        .take(3)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .map(|event| {
            format!(
                "{}:{}:{}",
                event.tool_name,
                if matches!(event.status, ObservationStatus::Failed) {
                    "failed"
                } else {
                    "ok"
                },
                clip(&event.output, 600)
            )
        })
        .collect::<Vec<_>>();
    Some(entries.join(" || "))
}

fn is_benchmark_seed_candidate(
    record: &DogfoodRecord,
    outcome_filter: Option<DogfoodOutcome>,
) -> bool {
    if let Some(filter) = outcome_filter {
        if record.outcome != filter {
            return false;
        }
    } else if matches!(record.outcome, DogfoodOutcome::Success) {
        return false;
    }

    record
        .benchmark_seed_observations
        .as_deref()
        .map(str::trim)
        .is_some_and(|value| !value.is_empty())
}

fn is_external_write_fixture_record(record: &DogfoodRecord) -> bool {
    record
        .notes
        .as_deref()
        .is_some_and(|notes| notes.contains("external-write-fixture"))
        && benchmark_case_category(record) == "write_validate"
}

fn render_benchmark_seed_export(
    records: &[DogfoodRecord],
    limit: usize,
    outcome_filter: Option<DogfoodOutcome>,
    repo_root: &Path,
) -> String {
    let mut out = String::new();
    out.push_str("# Generated benchmark seed cases from dogfood ledger\n");
    out.push_str("# Review and curate before appending to .dscode/benchmarks.txt.\n\n");

    let mut emitted = 0usize;
    for record in records.iter().rev() {
        if emitted >= limit || !is_benchmark_seed_candidate(record, outcome_filter) {
            continue;
        }
        let Some(seed_observations) = record.benchmark_seed_observations.as_deref() else {
            continue;
        };
        out.push_str(&format!(
            "# outcome={} timestamp={} tool_trace={} final_message={}\n",
            record.outcome.label(),
            record.timestamp_secs,
            clip(&record.tool_trace, 80),
            clip(&record.final_message, 96),
        ));
        out.push_str(&format!("name = \"{}\"\n", benchmark_case_name(record)));
        out.push_str(&format!("task = \"{}\"\n", manifest_escape(&record.task)));
        out.push_str(&format!(
            "category = \"{}\"\n",
            manifest_escape(&benchmark_case_category(record))
        ));
        if let Some(skill) = record.skill.as_deref() {
            out.push_str(&format!("skill = \"{}\"\n", manifest_escape(skill)));
        }
        if let Some(workdir) = manifest_workdir(&record.workdir, repo_root) {
            out.push_str(&format!("workdir = \"{}\"\n", manifest_escape(&workdir)));
        }
        out.push_str(&format!("budget = {}\n", record.budget));
        out.push_str(&format!(
            "notes = \"{}\"\n",
            manifest_escape(&format!(
                "Generated from dogfood outcome={} trace={}{}",
                record.outcome.label(),
                clip(&record.tool_trace, 80),
                record
                    .notes
                    .as_deref()
                    .map(|value| format!(" notes={}", clip(value, 48)))
                    .unwrap_or_default()
            ))
        ));
        out.push_str(&format!(
            "seed_observations = \"{}\"\n\n",
            manifest_escape(seed_observations)
        ));
        emitted += 1;
    }

    if emitted == 0 {
        out.push_str("# No matching failed/stuck/manual runs with replayable seed observations were found.\n");
    }

    out
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PromotedBenchmarkCase {
    name: String,
    block: String,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct PromotionPlan {
    cases: Vec<PromotedBenchmarkCase>,
    duplicates_skipped: usize,
    policy_skipped: usize,
    policy_skip_reasons: Vec<PolicySkipReasonCount>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PolicySkipReasonCount {
    reason_code: &'static str,
    reason_label: &'static str,
    count: usize,
    example_task: String,
}

fn build_promotion_plan(
    records: &[DogfoodRecord],
    existing: &[BenchmarkCaseSummary],
    limit: usize,
    outcome_filter: Option<DogfoodOutcome>,
    repo_root: &Path,
) -> PromotionPlan {
    let mut existing_names = existing
        .iter()
        .map(|case| case.name.clone())
        .collect::<HashSet<_>>();
    let mut existing_keys = existing
        .iter()
        .map(benchmark_summary_key)
        .collect::<HashSet<_>>();
    let mut cases = Vec::new();
    let mut duplicates_skipped = 0usize;
    let mut policy_skipped = 0usize;
    let mut policy_skip_reasons = BTreeMap::<&'static str, PolicySkipReasonCount>::new();

    for record in records.iter().rev() {
        if cases.len() >= limit || !is_benchmark_seed_candidate(record, outcome_filter) {
            continue;
        }
        if let Some(reason) = promotion_policy_rejection(record, outcome_filter) {
            policy_skipped += 1;
            policy_skip_reasons
                .entry(reason.code)
                .and_modify(|summary| summary.count += 1)
                .or_insert_with(|| PolicySkipReasonCount {
                    reason_code: reason.code,
                    reason_label: reason.label,
                    count: 1,
                    example_task: record.task.clone(),
                });
            continue;
        }
        let Some(seed_observations) = record.benchmark_seed_observations.as_deref() else {
            continue;
        };
        let skill = record.skill.clone();
        let workdir = manifest_workdir(&record.workdir, repo_root);
        let category = benchmark_case_category(record);
        let key = benchmark_identity_key(
            &record.task,
            Some(category.as_ref()),
            skill.as_deref(),
            workdir.as_deref(),
            seed_observations,
        );
        if existing_keys.contains(&key) {
            duplicates_skipped += 1;
            continue;
        }
        existing_keys.insert(key);
        let name = unique_benchmark_case_name(benchmark_case_name(record), &mut existing_names);
        cases.push(PromotedBenchmarkCase {
            block: render_promoted_case_block(record, &name, workdir.as_deref(), seed_observations),
            name,
        });
    }

    PromotionPlan {
        cases,
        duplicates_skipped,
        policy_skipped,
        policy_skip_reasons: policy_skip_reasons.into_values().collect(),
    }
}

fn append_promoted_cases(path: &Path, cases: &[PromotedBenchmarkCase]) -> AppResult<()> {
    if cases.is_empty() {
        return Ok(());
    }
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|error| {
            app_error(format!(
                "failed to open benchmark manifest {} for append: {error}",
                path.display()
            ))
        })?;
    let needs_separator = fs::metadata(path)
        .map(|metadata| metadata.len() > 0)
        .unwrap_or(false);
    if needs_separator {
        writeln!(file)?;
    }
    for (index, case) in cases.iter().enumerate() {
        if index > 0 {
            writeln!(file)?;
        }
        write!(file, "{}", case.block)?;
    }
    Ok(())
}

fn render_promoted_case_block(
    record: &DogfoodRecord,
    name: &str,
    workdir: Option<&str>,
    seed_observations: &str,
) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "# promoted from dogfood outcome={} timestamp={} tool_trace={} final_message={}\n",
        record.outcome.label(),
        record.timestamp_secs,
        clip(&record.tool_trace, 80),
        clip(&record.final_message, 96),
    ));
    out.push_str(&format!("name = \"{}\"\n", manifest_escape(name)));
    out.push_str(&format!("task = \"{}\"\n", manifest_escape(&record.task)));
    out.push_str(&format!(
        "category = \"{}\"\n",
        manifest_escape(&benchmark_case_category(record))
    ));
    if let Some(skill) = record.skill.as_deref() {
        out.push_str(&format!("skill = \"{}\"\n", manifest_escape(skill)));
    }
    if let Some(workdir) = workdir {
        out.push_str(&format!("workdir = \"{}\"\n", manifest_escape(workdir)));
    }
    out.push_str(&format!("budget = {}\n", record.budget));
    out.push_str(&format!(
        "notes = \"{}\"\n",
        manifest_escape(&format!(
            "Promoted from dogfood outcome={} trace={}{}",
            record.outcome.label(),
            clip(&record.tool_trace, 80),
            record
                .notes
                .as_deref()
                .map(|value| format!(" notes={}", clip(value, 48)))
                .unwrap_or_default()
        ))
    ));
    out.push_str(&format!(
        "seed_observations = \"{}\"\n",
        manifest_escape(seed_observations)
    ));
    out
}

fn benchmark_case_name(record: &DogfoodRecord) -> String {
    let slug = slugify(&record.task, 32);
    format!("dogfood-{}-{}", record.outcome.label(), slug)
}

fn benchmark_case_category(record: &DogfoodRecord) -> Cow<'_, str> {
    resolved_benchmark_category(
        record.benchmark_category.as_deref(),
        &record.task,
        &record.tool_trace,
        record.failed_tool_calls,
        record.repeated_call_failures,
        record.used_subagent,
        record.benchmark_seed_observations.as_deref(),
    )
}

pub(crate) fn resolved_benchmark_category<'a>(
    stored: Option<&'a str>,
    task: &'a str,
    tool_trace: &'a str,
    failed_tool_calls: u64,
    repeated_call_failures: u64,
    used_subagent: bool,
    benchmark_seed_observations: Option<&'a str>,
) -> Cow<'a, str> {
    let inferred = infer_benchmark_category(
        task,
        tool_trace,
        failed_tool_calls,
        repeated_call_failures,
        used_subagent,
        benchmark_seed_observations,
    );
    match stored {
        Some("read_only") if inferred == "recovery" => Cow::Borrowed(inferred),
        Some("write_validate") if inferred == "recovery" && !tool_trace.contains("apply_patch") => {
            Cow::Borrowed(inferred)
        }
        Some("planning") if inferred != "planning" && !task_looks_like_planning(task) => {
            Cow::Borrowed(inferred)
        }
        Some(stored) => Cow::Borrowed(stored),
        None => Cow::Borrowed(inferred),
    }
}

pub(crate) fn infer_benchmark_category<'a>(
    task: &'a str,
    tool_trace: &'a str,
    failed_tool_calls: u64,
    repeated_call_failures: u64,
    used_subagent: bool,
    benchmark_seed_observations: Option<&'a str>,
) -> &'static str {
    let task_lower = task.to_ascii_lowercase();
    let trace_lower = tool_trace.to_ascii_lowercase();
    if task_looks_like_pr_workflow(&task_lower) {
        return "pr_workflow";
    }
    if benchmark_seed_observations.is_some_and(|seed| seed.contains("recovery_hint:"))
        || repeated_call_failures > 0
        || task_looks_like_recovery(&task_lower)
        || task_looks_like_failure_diagnosis(&task_lower)
    {
        return "recovery";
    }
    if task_looks_like_write_validate(&task_lower)
        || trace_lower.contains("apply_patch")
        || trace_lower.contains("run_shell")
    {
        return "write_validate";
    }
    if failed_tool_calls > 0 {
        return "recovery";
    }
    if task_looks_like_subagent(&task_lower) {
        return "subagent";
    }
    if task_looks_like_planning(&task_lower)
        || (trace_lower.contains("todo_write")
            && !trace_lower.contains("read_file")
            && !trace_lower.contains("list_files")
            && !trace_lower.contains("search_text")
            && !trace_lower.contains("dispatch_subagent")
            && !used_subagent)
    {
        return "planning";
    }
    "read_only"
}

fn task_looks_like_planning(task: &str) -> bool {
    let task_lower = task.to_ascii_lowercase();
    task_lower.starts_with("plan ")
        || task_lower.contains(" plan ")
        || task_lower.contains(" planning")
        || task_lower.contains("before acting")
        || task_lower.contains("execution steps")
        || task_lower.contains("step-by-step plan")
}

fn task_looks_like_subagent(task: &str) -> bool {
    task.contains("dispatch_subagent")
        || task.contains("subagent")
        || task.contains("child loop")
        || task.contains("parent loop")
}

fn task_looks_like_recovery(task: &str) -> bool {
    task.contains("if there are no matches")
        || task.contains("if there are no match")
        || task.contains("if no matches")
        || task.contains("before retrying the command")
        || task.contains("before retrying the read")
        || task.contains("broaden the lookup")
}

fn task_looks_like_write_validate(task: &str) -> bool {
    let has_explicit_edit =
        task.contains("replace ") && task.contains(" with ") && task.contains(" in ");
    let asks_validation = task.contains("validate")
        || task.contains("rerun")
        || task.contains("tests pass")
        || task.contains("cargo test")
        || task.contains("pytest")
        || task.contains("npm test");
    has_explicit_edit && asks_validation
}

fn task_looks_like_failure_diagnosis(task: &str) -> bool {
    let task_lower = task.to_ascii_lowercase();
    task_lower.contains("investigate why")
        || task_lower.contains("diagnose")
        || task_lower.contains("reproduce")
        || task_lower.contains("inspect the failing")
        || task_lower.contains("before retrying")
}

fn task_looks_like_pr_workflow(task: &str) -> bool {
    task.contains("pull request")
        || task.contains("review feedback")
        || task.contains("ci job")
        || task.contains("failed ci")
        || task.contains("pr #")
        || task.contains("github pr")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PolicyRejection {
    code: &'static str,
    label: &'static str,
}

fn promotion_policy_rejection(
    record: &DogfoodRecord,
    outcome_filter: Option<DogfoodOutcome>,
) -> Option<PolicyRejection> {
    if record.tool_trace == "none" || record.tool_calls == 0 {
        return Some(PolicyRejection {
            code: "no_tool_trace",
            label: "missing real tool trace",
        });
    }
    if record.tool_calls > 8 {
        return Some(PolicyRejection {
            code: "tool_trace_too_long",
            label: "tool trace too long (>8 calls)",
        });
    }

    if outcome_filter.is_none()
        && !matches!(
            record.outcome,
            DogfoodOutcome::Failed | DogfoodOutcome::Stuck
        )
    {
        return Some(PolicyRejection {
            code: "manual_requires_explicit_filter",
            label: "manual outcome requires --outcome manual",
        });
    }

    if !(record.failed_tool_calls > 0
        || record.repeated_call_failures > 0
        || record.manual_intervention)
    {
        return Some(PolicyRejection {
            code: "missing_failure_signal",
            label: "missing failed/stuck/manual signal",
        });
    }

    None
}

fn unique_benchmark_case_name(base: String, existing_names: &mut HashSet<String>) -> String {
    if existing_names.insert(base.clone()) {
        return base;
    }
    for suffix in 2..=9999 {
        let candidate = format!("{base}-{suffix}");
        if existing_names.insert(candidate.clone()) {
            return candidate;
        }
    }
    format!("{base}-overflow")
}

fn benchmark_summary_key(case: &BenchmarkCaseSummary) -> String {
    benchmark_identity_key(
        &case.task,
        Some(case.category.as_str()),
        case.skill.as_deref(),
        case.workdir.as_deref(),
        case.seed_observations.as_deref().unwrap_or(""),
    )
}

fn benchmark_identity_key(
    task: &str,
    category: Option<&str>,
    skill: Option<&str>,
    workdir: Option<&str>,
    seed_observations: &str,
) -> String {
    format!(
        "{}\u{1f}{}\u{1f}{}\u{1f}{}\u{1f}{}",
        task.trim(),
        category.unwrap_or(""),
        skill.unwrap_or(""),
        workdir.unwrap_or(""),
        seed_observations.trim()
    )
}

fn slugify(text: &str, max_len: usize) -> String {
    let mut out = String::new();
    let mut last_was_dash = false;
    for ch in text.chars() {
        let mapped = if ch.is_ascii_alphanumeric() {
            Some(ch.to_ascii_lowercase())
        } else if ch.is_whitespace() || matches!(ch, '-' | '_' | '/' | ':') {
            Some('-')
        } else {
            None
        };
        let Some(ch) = mapped else {
            continue;
        };
        if ch == '-' {
            if out.is_empty() || last_was_dash {
                continue;
            }
            last_was_dash = true;
        } else {
            last_was_dash = false;
        }
        out.push(ch);
        if out.len() >= max_len {
            break;
        }
    }
    out.trim_matches('-').to_string()
}

fn manifest_workdir(workdir: &str, repo_root: &Path) -> Option<String> {
    let path = Path::new(workdir);
    if path == repo_root {
        return None;
    }
    if let Ok(relative) = path.strip_prefix(repo_root) {
        let value = relative.display().to_string();
        if value.is_empty() || value == "." {
            None
        } else {
            Some(value)
        }
    } else if path.is_relative() {
        Some(workdir.to_string())
    } else {
        None
    }
}

fn manifest_escape(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('\n', "\\n")
        .replace('"', "\\\"")
}

fn read_string<'a>(root: &'a BTreeMap<String, JsonValue>, key: &str) -> AppResult<&'a str> {
    root.get(key)
        .and_then(json_as_string)
        .ok_or_else(|| app_error(format!("dogfood record missing string `{key}`")))
}

fn read_optional_string<'a>(root: &'a BTreeMap<String, JsonValue>, key: &str) -> Option<&'a str> {
    match root.get(key) {
        Some(JsonValue::Null) | None => None,
        Some(value) => json_as_string(value),
    }
}

fn read_u64(root: &BTreeMap<String, JsonValue>, key: &str) -> AppResult<u64> {
    root.get(key)
        .and_then(json_as_u64)
        .ok_or_else(|| app_error(format!("dogfood record missing numeric `{key}`")))
}

fn read_bool(root: &BTreeMap<String, JsonValue>, key: &str) -> AppResult<bool> {
    match root.get(key) {
        Some(JsonValue::Bool(value)) => Ok(*value),
        _ => Err(app_error(format!("dogfood record missing boolean `{key}`"))),
    }
}

fn read_optional_bool(root: &BTreeMap<String, JsonValue>, key: &str) -> Option<bool> {
    match root.get(key) {
        Some(JsonValue::Bool(value)) => Some(*value),
        _ => None,
    }
}

fn unix_now_secs() -> AppResult<u64> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| app_error(format!("system clock error: {error}")))?
        .as_secs())
}

fn first_non_empty_line(text: &str) -> Option<&str> {
    text.lines().map(str::trim).find(|line| !line.is_empty())
}

fn clip(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }
    let head: String = value.chars().take(max_chars).collect();
    format!("{head}…")
}

fn escape_table(value: &str) -> String {
    value.replace('|', "\\|")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::loop_runtime::ToolEvent;
    use crate::model::protocol::TokenUsage;
    use crate::util::json::{json_as_array, json_as_object, json_as_string};
    use std::fs;

    fn temp_test_dir(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        std::env::temp_dir().join(format!("deepseek-dogfood-{name}-{nanos}"))
    }

    fn test_record(timestamp_secs: u64, category: &str, outcome: DogfoodOutcome) -> DogfoodRecord {
        DogfoodRecord {
            version: 1,
            timestamp_secs,
            duration_ms: 20,
            task: "replace `a - b` with `a + b` in src/lib.rs and validate with cargo test"
                .to_string(),
            skill: None,
            budget: 6,
            model: "x".to_string(),
            model_transport: MODEL_TRANSPORT_UNKNOWN.to_string(),
            workdir: "/tmp/external-repo-copy".to_string(),
            outcome,
            manual_intervention: false,
            notes: None,
            tool_calls: 3,
            failed_tool_calls: 0,
            repeated_call_failures: 0,
            diagnostic_expected_failure: false,
            used_subagent: false,
            final_message: "tests pass".to_string(),
            tool_trace: "apply_patch -> git_diff -> run_shell".to_string(),
            error_kind: None,
            benchmark_category: Some(category.to_string()),
            benchmark_seed_observations: None,
        }
    }

    #[test]
    fn derive_default_outcome_prefers_stuck_over_failed() {
        assert!(matches!(
            derive_default_outcome(1, 1, false, false),
            DogfoodOutcome::Stuck
        ));
        assert!(matches!(
            derive_default_outcome(1, 0, false, false),
            DogfoodOutcome::Failed
        ));
        assert!(matches!(
            derive_default_outcome(0, 0, false, false),
            DogfoodOutcome::Success
        ));
    }

    #[test]
    fn derive_default_outcome_treats_expected_failure_diagnosis_as_success() {
        assert!(matches!(
            derive_default_outcome(1, 0, true, false),
            DogfoodOutcome::Success
        ));
    }

    #[test]
    fn derive_default_outcome_treats_recovered_validation_as_success() {
        assert!(matches!(
            derive_default_outcome(1, 0, false, true),
            DogfoodOutcome::Success
        ));
        assert!(matches!(
            derive_default_outcome(1, 1, false, true),
            DogfoodOutcome::Success
        ));
    }

    #[test]
    fn dogfood_error_environment_transport_detection_matches_network_failures() {
        let dns_error = app_error(
            "deepseek openai stream failed (exit Some(6)): curl: (6) Could not resolve host: api.deepseek.com",
        );
        assert!(dogfood_error_is_environment_transport_failure(
            dns_error.as_ref()
        ));

        let timeout_error = app_error("curl: (28) Connection timed out after 30000 milliseconds");
        assert!(dogfood_error_is_environment_transport_failure(
            timeout_error.as_ref()
        ));

        let agent_error = app_error("apply_patch failed: failed hunk");
        assert!(!dogfood_error_is_environment_transport_failure(
            agent_error.as_ref()
        ));
    }

    #[test]
    fn resolve_run_workdir_defaults_to_repo_root() {
        let repo_root = temp_test_dir("repo-root-default");
        fs::create_dir_all(&repo_root).expect("repo root");

        let resolved = super::resolve_run_workdir(&repo_root, None).expect("repo root");
        assert_eq!(resolved, repo_root);

        fs::remove_dir_all(&resolved).ok();
    }

    #[test]
    fn resolve_run_workdir_resolves_relative_path_under_repo_root() {
        let repo_root = temp_test_dir("repo-root-relative");
        let fixture = repo_root.join("fixtures").join("mini");
        fs::create_dir_all(&fixture).expect("fixture dir");

        let resolved =
            super::resolve_run_workdir(&repo_root, Some("fixtures/mini")).expect("fixture path");
        assert_eq!(resolved, fixture);

        fs::remove_dir_all(&repo_root).ok();
    }

    #[test]
    fn resolve_run_workdir_rejects_missing_directory() {
        let repo_root = temp_test_dir("repo-root-missing");
        fs::create_dir_all(&repo_root).expect("repo root");

        let error = super::resolve_run_workdir(&repo_root, Some("fixtures/missing")).unwrap_err();
        assert!(error
            .to_string()
            .contains("dogfood workdir does not exist or is not a directory"));

        fs::remove_dir_all(&repo_root).ok();
    }

    #[test]
    fn external_fixture_workdir_requires_external_git_repo() {
        let repo_root = temp_test_dir("external-fixture-repo-root");
        let internal_fixture = repo_root.join("fixtures").join("mini");
        fs::create_dir_all(internal_fixture.join(".git")).expect("internal git fixture");

        let error =
            super::validate_external_fixture_workdir(&repo_root, &internal_fixture).unwrap_err();
        assert!(error
            .to_string()
            .contains("external fixture workdir must be outside this repository"));

        let external_root = temp_test_dir("external-fixture-repo");
        fs::create_dir_all(&external_root).expect("external root");
        let missing_git =
            super::validate_external_fixture_workdir(&repo_root, &external_root).unwrap_err();
        assert!(missing_git
            .to_string()
            .contains("must be a git repository or worktree"));

        fs::create_dir_all(external_root.join(".git")).expect("external git metadata");
        super::validate_external_fixture_workdir(&repo_root, &external_root)
            .expect("external git fixture should pass");

        fs::remove_dir_all(&repo_root).ok();
        fs::remove_dir_all(&external_root).ok();
    }

    #[test]
    fn external_fixture_task_must_be_write_validate() {
        let error = super::validate_external_fixture_task("inspect repository layout").unwrap_err();
        assert!(error
            .to_string()
            .contains("must describe an edit and validation command"));

        super::validate_external_fixture_task(
            "replace `a - b` with `a + b` in src/lib.rs and validate with cargo test",
        )
        .expect("write validate task should pass");
    }

    #[test]
    fn external_fixture_extracts_validation_command() {
        let command = super::external_fixture_validation_command(
            "replace `a - b` with `a + b` in src/lib.rs and validate with cargo test.",
        )
        .expect("validation command");
        assert_eq!(command, "cargo test");

        let error = super::external_fixture_validation_command(
            "replace `a - b` with `a + b` in src/lib.rs",
        )
        .unwrap_err();
        assert!(error.to_string().contains("validate with <command>"));
    }

    #[test]
    fn external_fixture_requires_online_transport_unless_rehearsal() {
        let error =
            super::validate_external_fixture_model_transport(MODEL_TRANSPORT_OFFLINE, false)
                .unwrap_err();
        assert!(error.to_string().contains("requires online model-backed"));

        super::validate_external_fixture_model_transport(MODEL_TRANSPORT_OFFLINE, true)
            .expect("explicit offline rehearsal should pass");
        super::validate_external_fixture_model_transport(MODEL_TRANSPORT_ONLINE, false)
            .expect("online model-backed transport should pass");
    }

    #[test]
    fn external_fixture_notes_are_marked_for_reports() {
        assert_eq!(
            super::external_fixture_notes(Some("release evidence")),
            "external-write-fixture; release evidence"
        );
        assert_eq!(
            super::external_fixture_notes(Some("  ")),
            "external-write-fixture"
        );
    }

    #[test]
    fn record_json_round_trip_preserves_key_fields() {
        let record = DogfoodRecord {
            version: 1,
            timestamp_secs: 42,
            duration_ms: 88,
            task: "inspect planner".to_string(),
            skill: Some("research".to_string()),
            budget: 6,
            model: "deepseek-v4-pro".to_string(),
            model_transport: MODEL_TRANSPORT_ONLINE.to_string(),
            workdir: "/tmp/demo".to_string(),
            outcome: DogfoodOutcome::Manual,
            manual_intervention: true,
            notes: Some("needed one retry".to_string()),
            tool_calls: 4,
            failed_tool_calls: 1,
            repeated_call_failures: 0,
            diagnostic_expected_failure: false,
            used_subagent: true,
            final_message: "done".to_string(),
            tool_trace: "todo_write -> search_text".to_string(),
            error_kind: Some("tool_failure".to_string()),
            benchmark_category: Some("planning".to_string()),
            benchmark_seed_observations: Some("search_text:failed:no matches".to_string()),
        };
        let decoded = DogfoodRecord::from_json_line(&record.to_json_line()).unwrap();
        assert_eq!(decoded.task, "inspect planner");
        assert_eq!(decoded.skill.as_deref(), Some("research"));
        assert!(matches!(decoded.outcome, DogfoodOutcome::Manual));
        assert!(decoded.manual_intervention);
        assert_eq!(decoded.tool_trace, "todo_write -> search_text");
        assert_eq!(decoded.error_kind.as_deref(), Some("tool_failure"));
        assert_eq!(decoded.model_transport, MODEL_TRANSPORT_ONLINE);
        assert!(!decoded.diagnostic_expected_failure);
        assert_eq!(decoded.benchmark_category.as_deref(), Some("recovery"));
        assert_eq!(
            decoded.benchmark_seed_observations.as_deref(),
            Some("search_text:failed:no matches")
        );
    }

    #[test]
    fn record_json_round_trip_backfills_category_for_legacy_rows() {
        let decoded = DogfoodRecord::from_json_line(
            "{\"version\":1,\"timestamp_secs\":42,\"duration_ms\":88,\"task\":\"inspect planner\",\"skill\":\"research\",\"budget\":6,\"model\":\"deepseek-v4-pro\",\"workdir\":\"/tmp/demo\",\"outcome\":\"manual\",\"manual_intervention\":true,\"notes\":\"needed one retry\",\"tool_calls\":4,\"failed_tool_calls\":1,\"repeated_call_failures\":0,\"used_subagent\":true,\"final_message\":\"done\",\"tool_trace\":\"todo_write -> search_text\",\"error_kind\":\"tool_failure\",\"benchmark_seed_observations\":\"search_text:failed:no matches\"}"
        )
        .unwrap();
        assert_eq!(decoded.benchmark_category.as_deref(), Some("recovery"));
        assert_eq!(decoded.model_transport, MODEL_TRANSPORT_UNKNOWN);
    }

    #[test]
    fn render_report_includes_core_rates() {
        let records = vec![
            DogfoodRecord {
                version: 1,
                timestamp_secs: 1,
                duration_ms: 10,
                task: "one".to_string(),
                skill: None,
                budget: 4,
                model: "x".to_string(),
                model_transport: MODEL_TRANSPORT_UNKNOWN.to_string(),
                workdir: ".".to_string(),
                outcome: DogfoodOutcome::Success,
                manual_intervention: false,
                notes: None,
                tool_calls: 2,
                failed_tool_calls: 0,
                repeated_call_failures: 0,
                diagnostic_expected_failure: false,
                used_subagent: false,
                final_message: "ok".to_string(),
                tool_trace: "list_files".to_string(),
                error_kind: None,
                benchmark_category: Some("read_only".to_string()),
                benchmark_seed_observations: None,
            },
            DogfoodRecord {
                version: 1,
                timestamp_secs: 2,
                duration_ms: 12,
                task: "two".to_string(),
                skill: None,
                budget: 4,
                model: "x".to_string(),
                model_transport: MODEL_TRANSPORT_UNKNOWN.to_string(),
                workdir: ".".to_string(),
                outcome: DogfoodOutcome::Stuck,
                manual_intervention: true,
                notes: Some("needed manual help".to_string()),
                tool_calls: 3,
                failed_tool_calls: 1,
                repeated_call_failures: 1,
                diagnostic_expected_failure: false,
                used_subagent: false,
                final_message: "stuck".to_string(),
                tool_trace: "list_files -> list_files".to_string(),
                error_kind: None,
                benchmark_category: Some("recovery".to_string()),
                benchmark_seed_observations: Some(
                    "list_files:failed:repeated identical tool call detected".to_string(),
                ),
            },
        ];
        let report = render_report(Path::new(".dscode/dogfood/ledger.jsonl"), &records, 20);
        assert!(report.contains("# DeepSeekCode Dogfood Report"));
        assert!(report.contains("Success rate: 1/2 (50.0%)"));
        assert!(report.contains("Diagnostic expected-failure rate: 0/2 (0.0%)"));
        assert!(report.contains("Stuck rate: 1/2 (50.0%)"));
        assert!(report.contains("Manual intervention rate: 1/2 (50.0%)"));
        assert!(report.contains("Benchmark seed candidates: 1"));
        assert!(report.contains("## Category Breakdown"));
        assert!(report.contains("## Category Trend"));
        assert!(report.contains("Status: insufficient history"));
        assert!(report.contains("| read_only | 1 | 1/1 (100.0%) | 0/1 (0.0%) | 0/1 (0.0%) | 0/1 (0.0%) | 0/1 (0.0%) | 2.00 | 0 |"));
        assert!(report.contains("| recovery | 1 | 0/1 (0.0%) | 0/1 (0.0%) | 0/1 (0.0%) | 1/1 (100.0%) | 1/1 (100.0%) | 3.00 | 1 |"));
        assert!(report.contains("| Timestamp | Category | Outcome | Transport | Manual | Budget | Tool Calls | Failed Tools | Workdir | Task | Notes |"));
    }

    #[test]
    fn render_report_includes_category_trend_deltas() {
        let mut records = Vec::new();
        for index in 0..10u64 {
            let recent = index >= 5;
            let success = recent || index < 3;
            let tool_calls = if recent { 2 } else { 4 };
            records.push(DogfoodRecord {
                version: 1,
                timestamp_secs: index + 1,
                duration_ms: 10,
                task: format!("inspect file {index}"),
                skill: None,
                budget: 4,
                model: "x".to_string(),
                model_transport: MODEL_TRANSPORT_UNKNOWN.to_string(),
                workdir: ".".to_string(),
                outcome: if success {
                    DogfoodOutcome::Success
                } else {
                    DogfoodOutcome::Failed
                },
                manual_intervention: false,
                notes: None,
                tool_calls,
                failed_tool_calls: 0,
                repeated_call_failures: 0,
                diagnostic_expected_failure: false,
                used_subagent: false,
                final_message: "done".to_string(),
                tool_trace: "list_files -> read_file".to_string(),
                error_kind: None,
                benchmark_category: Some("read_only".to_string()),
                benchmark_seed_observations: None,
            });
        }
        let report = render_report(Path::new(".dscode/dogfood/ledger.jsonl"), &records, 20);
        assert!(report.contains("Compared windows: recent=5 previous=5"));
        assert!(report.contains("| read_only | 5 | 5 | 5/5 (100.0%) | 3/5 (60.0%) | +40.0 | 2.00 | 4.00 | -2.00 | 0 | 0 |"));
    }

    #[test]
    fn render_report_labels_expected_failure_diagnosis_separately() {
        let records = vec![DogfoodRecord {
            version: 1,
            timestamp_secs: 7,
            duration_ms: 11,
            task: "investigate why npm test fails in the JavaScript CLI and inspect the failing test file before retrying".to_string(),
            skill: Some("debug".to_string()),
            budget: 4,
            model: "x".to_string(),
            model_transport: MODEL_TRANSPORT_UNKNOWN.to_string(),
            workdir: ".".to_string(),
            outcome: DogfoodOutcome::Success,
            manual_intervention: false,
            notes: Some("diagnosis only".to_string()),
            tool_calls: 2,
            failed_tool_calls: 1,
            repeated_call_failures: 0,
            diagnostic_expected_failure: true,
            used_subagent: false,
            final_message: "read back the failing test file".to_string(),
            tool_trace: "run_shell -> read_file".to_string(),
            error_kind: None,
            benchmark_category: Some("recovery".to_string()),
            benchmark_seed_observations: Some(
                "run_shell:ok:meta.result=failed || read_file:ok:test('route benchmark stays stable')"
                    .to_string(),
            ),
        }];

        let report = render_report(Path::new(".dscode/dogfood/ledger.jsonl"), &records, 20);
        assert!(report.contains("Diagnostic expected-failure rate: 1/1 (100.0%)"));
        assert!(report.contains("| recovery | 1 | 1/1 (100.0%) | 1/1 (100.0%) | 0/1 (0.0%) | 0/1 (0.0%) | 0/1 (0.0%) | 2.00 | 0 |"));
        assert!(report.contains("| 7 | recovery | diagnostic | unknown | no | 4 | 2 | 1 | . | investigate why npm test fails in the JavaScript CLI"));
    }

    #[test]
    fn render_report_counts_external_write_fixture_evidence() {
        let records = vec![DogfoodRecord {
            version: 1,
            timestamp_secs: 8,
            duration_ms: 20,
            task: "replace `a - b` with `a + b` in src/lib.rs and validate with cargo test"
                .to_string(),
            skill: None,
            budget: 6,
            model: "x".to_string(),
            model_transport: MODEL_TRANSPORT_UNKNOWN.to_string(),
            workdir: "/tmp/external-repo-copy".to_string(),
            outcome: DogfoodOutcome::Success,
            manual_intervention: false,
            notes: Some("external-write-fixture; disposable repo".to_string()),
            tool_calls: 3,
            failed_tool_calls: 0,
            repeated_call_failures: 0,
            diagnostic_expected_failure: false,
            used_subagent: false,
            final_message: "tests pass".to_string(),
            tool_trace: "apply_patch -> git_diff -> run_shell".to_string(),
            error_kind: None,
            benchmark_category: Some("write_validate".to_string()),
            benchmark_seed_observations: None,
        }];

        let report = render_report(Path::new(".dscode/dogfood/ledger.jsonl"), &records, 20);
        assert!(report.contains("External write fixtures: 1/1 (100.0%)"));
    }

    #[test]
    fn render_report_counts_model_backed_evidence() {
        let mut online = test_record(8, "write_validate", DogfoodOutcome::Success);
        online.model_transport = MODEL_TRANSPORT_ONLINE.to_string();
        let offline = test_record(9, "write_validate", DogfoodOutcome::Success);

        let report = render_report(
            Path::new(".dscode/dogfood/ledger.jsonl"),
            &[online, offline],
            20,
        );

        assert!(report.contains("Model-backed runs: 1/1 (100.0%)"));
        assert!(report.contains("| 8 | write_validate | success | online | no |"));
        assert!(report.contains("| 9 | write_validate | success | unknown | no |"));
    }

    #[test]
    fn report_requirements_pass_with_external_and_category_evidence() {
        let mut external = test_record(8, "write_validate", DogfoodOutcome::Success);
        external.notes = Some("external-write-fixture; disposable repo".to_string());
        external.model_transport = MODEL_TRANSPORT_ONLINE.to_string();
        let mut recovery = test_record(6, "recovery", DogfoodOutcome::Success);
        recovery.model_transport = MODEL_TRANSPORT_ONLINE.to_string();
        let records = vec![
            recovery,
            test_record(7, "write_validate", DogfoodOutcome::Success),
            external,
        ];
        let args = DogfoodReportArgs {
            require_min_runs: Some(3),
            require_success_rate: Some(100.0),
            require_live_runs: Some(2),
            require_live_success_rate: Some(100.0),
            require_external_write_fixtures: Some(1),
            require_recent_clean: Some(3),
            require_categories: vec![DogfoodCategoryRequirement {
                category: "write_validate".to_string(),
                min_runs: 2,
                min_success_percent: 100.0,
            }],
            require_live_categories: vec![DogfoodCategoryRequirement {
                category: "recovery".to_string(),
                min_runs: 1,
                min_success_percent: 100.0,
            }],
            ..DogfoodReportArgs::default()
        };

        assert!(report_requirement_failures(&records, &args).is_empty());
    }

    #[test]
    fn report_requirements_fail_on_missing_live_evidence() {
        let mut manual = test_record(8, "write_validate", DogfoodOutcome::Failed);
        manual.manual_intervention = true;
        let mut online_failure = test_record(9, "recovery", DogfoodOutcome::Failed);
        online_failure.model_transport = MODEL_TRANSPORT_ONLINE.to_string();
        let records = vec![
            test_record(6, "recovery", DogfoodOutcome::Success),
            test_record(7, "write_validate", DogfoodOutcome::Success),
            manual,
            online_failure,
        ];
        let args = DogfoodReportArgs {
            require_min_runs: Some(5),
            require_success_rate: Some(90.0),
            require_live_runs: Some(2),
            require_live_success_rate: Some(90.0),
            require_external_write_fixtures: Some(1),
            require_recent_clean: Some(3),
            require_categories: vec![DogfoodCategoryRequirement {
                category: "write_validate".to_string(),
                min_runs: 3,
                min_success_percent: 90.0,
            }],
            require_live_categories: vec![DogfoodCategoryRequirement {
                category: "write_validate".to_string(),
                min_runs: 1,
                min_success_percent: 90.0,
            }],
            ..DogfoodReportArgs::default()
        };

        let failures = report_requirement_failures(&records, &args);
        assert!(failures
            .iter()
            .any(|failure| failure.contains("runs 4 below required minimum 5")));
        assert!(failures
            .iter()
            .any(|failure| failure.contains("overall success rate 50.0% below required 90.0%")));
        assert!(failures
            .iter()
            .any(|failure| failure.contains("model-backed runs 1 below required minimum 2")));
        assert!(failures.iter().any(|failure| {
            failure.contains("model-backed success rate 0.0% below required 90.0% (0/1)")
        }));
        assert!(failures.iter().any(|failure| failure
            .contains("successful external write fixtures 0 below required minimum 1")));
        assert!(failures.iter().any(|failure| failure
            .contains("recent clean window contains 2 failed, stuck, or manual records")));
        assert!(failures
            .iter()
            .any(|failure| failure
                .contains("category `write_validate` runs 2 below required minimum 3")));
        assert!(failures.iter().any(|failure| {
            failure.contains("model-backed category `write_validate` has 0 runs, required 1")
        }));
        assert!(failures.iter().any(|failure| failure
            .contains("category `write_validate` success rate 50.0% below required 90.0%")));
    }

    #[test]
    fn live_plan_recommends_replayable_category_cases() {
        let mut online = test_record(10, "write_validate", DogfoodOutcome::Success);
        online.model_transport = MODEL_TRANSPORT_ONLINE.to_string();
        let records = vec![online, test_record(11, "recovery", DogfoodOutcome::Success)];
        let summaries = vec![
            BenchmarkCaseSummary {
                name: "fixture-write-validate-rust-mini".to_string(),
                task: "replace `a - b` with `a + b` in src/lib.rs and validate with cargo test"
                    .to_string(),
                category: "write_validate".to_string(),
                skill: None,
                workdir: Some("fixtures/rust-write-mini".to_string()),
                isolate_workdir: true,
                budget: 6,
                notes: None,
                seed_observations: None,
            },
            BenchmarkCaseSummary {
                name: "seeded-write-validate".to_string(),
                task: "seeded".to_string(),
                category: "write_validate".to_string(),
                skill: None,
                workdir: Some("fixtures/rust-write-mini".to_string()),
                isolate_workdir: true,
                budget: 6,
                notes: None,
                seed_observations: Some("run_shell:failed:test failed".to_string()),
            },
            BenchmarkCaseSummary {
                name: "fixture-recover-write-validate-rust-mini".to_string(),
                task: "replace `a - b` with `a * b` in src/lib.rs and validate with cargo test"
                    .to_string(),
                category: "write_validate".to_string(),
                skill: None,
                workdir: Some("fixtures/rust-write-mini".to_string()),
                isolate_workdir: true,
                budget: 6,
                notes: Some(
                    "Real isolated Rust write+validate failure case for apply_patch -> git_diff -> run_shell -> read_file"
                        .to_string(),
                ),
                seed_observations: None,
            },
            BenchmarkCaseSummary {
                name: "fixture-recover-empty-search".to_string(),
                task: "recover from missing symbol search".to_string(),
                category: "recovery".to_string(),
                skill: None,
                workdir: Some("fixtures/rust-cli-mini".to_string()),
                isolate_workdir: true,
                budget: 6,
                notes: None,
                seed_observations: None,
            },
        ];
        let targets = vec![
            DogfoodCategoryRequirement {
                category: "write_validate".to_string(),
                min_runs: 2,
                min_success_percent: 90.0,
            },
            DogfoodCategoryRequirement {
                category: "recovery".to_string(),
                min_runs: 1,
                min_success_percent: 90.0,
            },
        ];

        let plan = build_live_plan(
            Path::new(".dscode/dogfood/ledger.jsonl"),
            Path::new(".dscode/benchmarks.txt"),
            &records,
            &summaries,
            MODEL_TRANSPORT_OFFLINE,
            4,
            90.0,
            &targets,
            3,
        );
        let text = render_live_plan_text(&plan);

        assert_eq!(plan.live_runs, 1);
        assert_eq!(plan.live_success, 1);
        assert!(text.contains("warning: current model config will not count"));
        assert!(text.contains("write_validate: live 1/2"));
        assert!(text.contains("replayable_unique 1; recommended_now 1"));
        assert!(text.contains("fixture-write-validate-rust-mini"));
        assert!(text.contains(
            "dry_run: deepseek dogfood live-run --manifest .dscode/benchmarks.txt --category write_validate --limit 1"
        ));
        assert!(text.contains(
            "execute: deepseek dogfood live-run --manifest .dscode/benchmarks.txt --category write_validate --limit 1 --execute"
        ));
        assert!(text.contains(
            "post_run_report_gate: deepseek dogfood report --limit 20 --require-live-runs 4 --require-live-success-rate 90 --require-live-category write_validate:2:90 --require-live-category recovery:1:90"
        ));
        assert!(!text.contains("dogfood replay-benchmark"));
        assert!(!text.contains("seeded-write-validate,"));
        assert!(!text.contains("fixture-recover-write-validate-rust-mini"));
        assert!(text.contains("recovery: live 0/1"));
    }

    #[test]
    fn live_plan_default_targets_include_mcp_loop_surface() {
        let targets = live_plan_targets(Vec::new());
        let mcp = targets
            .iter()
            .find(|target| target.category == "mcp")
            .expect("default live plan should require MCP loop-surface evidence");
        assert_eq!(mcp.min_runs, 3);
        assert_eq!(mcp.min_success_percent, 90.0);
    }

    #[test]
    fn live_plan_json_includes_targets_and_recommendations() {
        let summaries = vec![BenchmarkCaseSummary {
            name: "fixture-pr-retry-validate-rust-mini".to_string(),
            task: "Address PR feedback and validate with cargo test".to_string(),
            category: "pr_workflow".to_string(),
            skill: None,
            workdir: Some("fixtures/rust-write-mini".to_string()),
            isolate_workdir: true,
            budget: 8,
            notes: None,
            seed_observations: None,
        }];
        let targets = vec![DogfoodCategoryRequirement {
            category: "pr_workflow".to_string(),
            min_runs: 1,
            min_success_percent: 90.0,
        }];

        let plan = build_live_plan(
            Path::new(".dscode/dogfood/ledger.jsonl"),
            Path::new(".dscode/benchmarks.txt"),
            &[],
            &summaries,
            MODEL_TRANSPORT_ONLINE,
            1,
            90.0,
            &targets,
            2,
        );
        let json = render_live_plan_json(&plan);

        assert!(json.contains("\"model_transport\":\"online\""));
        assert!(json.contains("\"overall_needed_runs\":1"));
        assert!(json.contains("\"category\":\"pr_workflow\""));
        assert!(json.contains("\"recommended_cases\":[\"fixture-pr-retry-validate-rust-mini\"]"));
        assert!(json.contains(
            "\"live_run_command\":\"deepseek dogfood live-run --manifest .dscode/benchmarks.txt --category pr_workflow --limit 1\""
        ));
        assert!(json.contains(
            "\"live_run_execute_command\":\"deepseek dogfood live-run --manifest .dscode/benchmarks.txt --category pr_workflow --limit 1 --execute\""
        ));
        assert!(json.contains(
            "\"post_run_report_command\":\"deepseek dogfood report --limit 20 --require-live-runs 1 --require-live-success-rate 90 --require-live-category pr_workflow:1:90\""
        ));
        assert!(json.contains("\"require_live_runs\":1"));
        assert!(json.contains("\"require_live_categories\":[{\"category\":\"pr_workflow\",\"min_runs\":1,\"min_success_rate\":90.0}]"));
    }

    #[test]
    fn live_run_plan_json_is_machine_readable_dry_run() {
        let plan = LivePlan {
            ledger_path: PathBuf::from(".dscode/dogfood/ledger.jsonl"),
            manifest_path: PathBuf::from(".dscode/benchmarks.txt"),
            model_transport: MODEL_TRANSPORT_OFFLINE.to_string(),
            target_live_runs: 100,
            target_live_success_rate: 90.0,
            live_runs: 20,
            live_success: 19,
            category_plans: vec![LiveCategoryPlan {
                category: "write_validate".to_string(),
                target_runs: 25,
                target_success_rate: 90.0,
                live_runs: 3,
                live_success: 3,
                needed_runs: 22,
                replayable_cases: vec!["write-1".to_string(), "write-2".to_string()],
                recommended_cases: vec!["write-1".to_string(), "write-2".to_string()],
            }],
        };
        let requested = vec!["write_validate".to_string()];
        let selected = select_live_run_cases(&plan, &requested, 1);
        let json = render_live_run_plan_json(&plan, &requested, 1, &selected, None, None);

        assert!(json.contains("\"kind\":\"deepseek.dogfood.live_run_plan.v1\""));
        assert!(json.contains("\"model_transport\":\"offline\""));
        assert!(json.contains("\"online_ready\":false"));
        assert!(json.contains("\"execute_ready\":false"));
        assert!(json.contains("\"selected_count\":1"));
        assert!(json.contains(
            "\"selected_cases\":[{\"category\":\"write_validate\",\"name\":\"write-1\"}]"
        ));
        assert!(json.contains(
            "\"dry_run_command\":\"deepseek dogfood live-run --manifest .dscode/benchmarks.txt --category write_validate --limit 1 --json\""
        ));
        assert!(json.contains(
            "\"execute_command\":\"deepseek dogfood live-run --manifest .dscode/benchmarks.txt --category write_validate --limit 1 --execute\""
        ));
        assert!(json.contains(
            "\"post_run_report_command\":\"deepseek dogfood report --limit 100 --require-live-runs 100 --require-live-success-rate 90 --require-live-category write_validate:25:90\""
        ));
        assert!(json.contains("\"evidence_gate\":{\"command\":\"deepseek dogfood report --limit 100 --require-live-runs 100 --require-live-success-rate 90 --require-live-category write_validate:25:90\""));
        assert!(json.contains("dogfood live-run --execute requires an online model transport"));
    }

    #[test]
    fn live_run_plan_json_preserves_api_key_file_without_secret_value() {
        let plan = LivePlan {
            ledger_path: PathBuf::from(".dscode/dogfood/ledger.jsonl"),
            manifest_path: PathBuf::from(".dscode/benchmarks.txt"),
            model_transport: MODEL_TRANSPORT_ONLINE.to_string(),
            target_live_runs: 100,
            target_live_success_rate: 90.0,
            live_runs: 20,
            live_success: 19,
            category_plans: vec![LiveCategoryPlan {
                category: "write_validate".to_string(),
                target_runs: 25,
                target_success_rate: 90.0,
                live_runs: 3,
                live_success: 3,
                needed_runs: 22,
                replayable_cases: vec!["write-1".to_string()],
                recommended_cases: vec!["write-1".to_string()],
            }],
        };
        let requested = vec!["write_validate".to_string()];
        let selected = select_live_run_cases(&plan, &requested, 1);
        let api_key_file = "/tmp/deepseek dogfood.key";
        let evidence_out = "/tmp/deepseek live evidence.json";
        let secret = "dogfood-secret-placeholder";
        let json = render_live_run_plan_json(
            &plan,
            &requested,
            1,
            &selected,
            Some(api_key_file),
            Some(evidence_out),
        );

        assert!(json.contains("\"credential_source\":\"api_key_file\""));
        assert!(json.contains("\"api_key_file\":\"/tmp/deepseek dogfood.key\""));
        assert!(json.contains("\"evidence_out\":\"/tmp/deepseek live evidence.json\""));
        assert!(json.contains(
            "\"dry_run_command\":\"deepseek dogfood live-run --manifest .dscode/benchmarks.txt --api-key-file '/tmp/deepseek dogfood.key' --evidence-out '/tmp/deepseek live evidence.json' --category write_validate --limit 1 --json\""
        ));
        assert!(json.contains(
            "\"execute_command\":\"deepseek dogfood live-run --manifest .dscode/benchmarks.txt --api-key-file '/tmp/deepseek dogfood.key' --evidence-out '/tmp/deepseek live evidence.json' --category write_validate --limit 1 --execute\""
        ));
        assert!(json.contains(
            "\"post_run_report_command\":\"deepseek dogfood report --limit 100 --require-live-runs 100 --require-live-success-rate 90 --require-live-category write_validate:25:90\""
        ));
        assert!(!json.contains(secret));
    }

    #[test]
    fn live_run_evidence_summary_records_batch_delta_without_secret_value() {
        let root = temp_test_dir("live-evidence-summary");
        let ledger = root.join("ledger.jsonl");
        let mut before_record = test_record(10, "write_validate", DogfoodOutcome::Success);
        before_record.model_transport = MODEL_TRANSPORT_ONLINE.to_string();
        let mut appended_record = test_record(11, "write_validate", DogfoodOutcome::Success);
        appended_record.model_transport = MODEL_TRANSPORT_ONLINE.to_string();
        appended_record.duration_ms = 42;
        let before_records = vec![before_record];
        let after_records = vec![before_records[0].clone(), appended_record.clone()];
        append_record(&ledger, &after_records[0]).unwrap();
        append_record(&ledger, &after_records[1]).unwrap();
        let plan = LivePlan {
            ledger_path: ledger.clone(),
            manifest_path: PathBuf::from(".dscode/benchmarks.txt"),
            model_transport: MODEL_TRANSPORT_ONLINE.to_string(),
            target_live_runs: 2,
            target_live_success_rate: 90.0,
            live_runs: 1,
            live_success: 1,
            category_plans: vec![LiveCategoryPlan {
                category: "write_validate".to_string(),
                target_runs: 2,
                target_success_rate: 90.0,
                live_runs: 1,
                live_success: 1,
                needed_runs: 1,
                replayable_cases: vec!["write-1".to_string()],
                recommended_cases: vec!["write-1".to_string()],
            }],
        };
        let selected = vec![LiveRunCase {
            category: "write_validate".to_string(),
            name: "write-1".to_string(),
        }];
        let api_key_file = "/tmp/deepseek dogfood.key";
        let evidence_out = "/tmp/deepseek-live-evidence.json";
        let secret = "dogfood-secret-placeholder";
        let case_evidence = vec![live_run_case_evidence_json(
            &selected[0],
            &[appended_record],
            None,
        )];
        let summary = live_run_evidence_summary_json(
            &plan,
            &["write_validate".to_string()],
            1,
            &selected,
            &case_evidence,
            &before_records,
            &after_records,
            Some(api_key_file),
            Some(evidence_out),
            true,
            dogfood_file_fingerprint_json(&ledger),
            None,
            None,
        );
        let json = json_value_to_string(&summary);

        assert!(json.contains("\"kind\":\"deepseek.dogfood.live_run_evidence.v1\""));
        assert!(json.contains("\"credential_source\":\"api_key_file\""));
        assert!(json.contains("\"evidence_out\":\"/tmp/deepseek-live-evidence.json\""));
        assert!(json.contains("\"before\":{\"live_runs\":1"));
        assert!(json.contains("\"after\":{\"live_runs\":2"));
        assert!(json.contains("\"appended_records\":1"));
        assert!(json.contains("\"appended_model_backed_records\":1"));
        assert!(json.contains("\"cases\":[{\"benchmark_category\":\"write_validate\""));
        assert!(json.contains("\"ledger_fingerprint\":{\"algorithm\":\"fnv1a64\""));
        assert!(json.contains("\"model_backed\":true"));
        assert!(
            json.contains("\"benchmark_gate\":{\"error\":null,\"passed\":true,\"requested\":true}")
        );
        assert!(json.contains("\"post_run_report_command\":\"deepseek dogfood report --limit 20 --require-live-runs 2 --require-live-success-rate 90 --require-live-category write_validate:2:90\""));
        assert!(!json.contains(secret));

        let root_json = parse_root_object(&json).unwrap();
        let verify_args = DogfoodLiveEvidenceArgs {
            file: Some(evidence_out.to_string()),
            require_benchmark_gate: true,
            ..DogfoodLiveEvidenceArgs::default()
        };
        let failures = live_evidence_failures(&root_json, &verify_args);
        assert!(failures.is_empty(), "{failures:?}");
        let report_gate_failures = live_evidence_report_gate_failures(&root_json).unwrap();
        assert!(report_gate_failures.is_empty(), "{report_gate_failures:?}");
        let mut tampered_root = root_json.clone();
        if let Some(JsonValue::Array(cases)) = tampered_root.get_mut("cases") {
            if let Some(JsonValue::Object(case)) = cases.first_mut() {
                case.insert(
                    "timestamp_secs".to_string(),
                    JsonValue::Number("999999".to_string()),
                );
            }
        }
        let tampered_failures = live_evidence_report_gate_failures(&tampered_root).unwrap();
        assert!(tampered_failures
            .iter()
            .any(|failure| failure.contains("was not found in ledger")));
        let mut tampered_fingerprint_root = root_json.clone();
        if let Some(JsonValue::Object(fingerprint)) =
            tampered_fingerprint_root.get_mut("ledger_fingerprint")
        {
            fingerprint.insert(
                "fnv1a64".to_string(),
                JsonValue::String("0000000000000000".to_string()),
            );
        }
        let tampered_fingerprint_failures =
            live_evidence_report_gate_failures(&tampered_fingerprint_root).unwrap();
        assert!(tampered_fingerprint_failures
            .iter()
            .any(|failure| failure.contains("fnv1a64 mismatch")));
        let verification = json_value_to_string(&live_evidence_verification_json(
            evidence_out,
            &root_json,
            &failures,
            true,
            &report_gate_failures,
        ));
        assert!(
            verification.contains("\"kind\":\"deepseek.dogfood.live_evidence_verification.v1\"")
        );
        assert!(verification.contains("\"ok\":true"));
        assert!(verification.contains("\"report_gate_passed\":true"));
        assert!(verification.contains("\"current_ledger_fingerprint\":{\"algorithm\":\"fnv1a64\""));

        let mut strict_args = DogfoodLiveEvidenceArgs::default();
        strict_args.require_appended_model_backed = Some(2);
        let failures = live_evidence_failures(&root_json, &strict_args);
        assert!(failures
            .iter()
            .any(|failure| failure.contains("below required 2")));

        let out = root.join("nested/evidence.json");
        write_live_run_evidence_summary(out.to_str().expect("utf8"), &summary).unwrap();
        let written = fs::read_to_string(&out).unwrap();
        assert!(written.contains("\"deepseek.dogfood.live_run_evidence.v1\""));
        let verification_out = root.join("nested/verification.json");
        live_evidence_command(DogfoodLiveEvidenceArgs {
            file: Some(out.display().to_string()),
            out: Some(verification_out.display().to_string()),
            require_benchmark_gate: true,
            require_report_gate: true,
            ..DogfoodLiveEvidenceArgs::default()
        })
        .unwrap();
        let verification_written = fs::read_to_string(&verification_out).unwrap();
        assert!(verification_written.contains("\"deepseek.dogfood.live_evidence_verification.v1\""));
        assert!(verification_written.contains("\"report_gate_passed\":true"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn live_evidence_requires_mcp_loop_surface_gate_when_requested() {
        let missing_root = parse_root_object(
            r#"{
                "kind":"deepseek.dogfood.live_run_evidence.v1",
                "completed":true,
                "model_transport":"online",
                "online_ready":true,
                "appended_model_backed_records":1,
                "post_run_report_command":"deepseek dogfood report --limit 20 --require-live-runs 1 --require-live-success-rate 90 --require-live-category write_validate:1:90",
                "cases":[{
                    "ledger_records_appended":1,
                    "model_backed":true,
                    "error":null,
                    "benchmark_category":"write_validate"
                }]
            }"#,
        )
        .unwrap();
        let args = DogfoodLiveEvidenceArgs {
            require_loop_surface_gate: true,
            ..DogfoodLiveEvidenceArgs::default()
        };
        let failures = live_evidence_failures(&missing_root, &args);
        assert!(failures.iter().any(|failure| failure
            .contains("post_run_report_command is missing MCP loop-surface live gate")));
        assert!(failures
            .iter()
            .any(|failure| failure.contains("live evidence has no MCP loop-surface case")));
        let verification = json_value_to_string(&live_evidence_verification_json(
            "live-evidence.json",
            &missing_root,
            &failures,
            false,
            &[],
        ));
        assert!(verification.contains("\"loop_surface_case_present\":false"));

        let passing_root = parse_root_object(
            r#"{
                "kind":"deepseek.dogfood.live_run_evidence.v1",
                "completed":true,
                "model_transport":"online",
                "online_ready":true,
                "appended_model_backed_records":1,
                "post_run_report_command":"deepseek dogfood report --limit 20 --require-live-runs 1 --require-live-success-rate 90 --require-live-category mcp:1:90",
                "cases":[{
                    "ledger_records_appended":1,
                    "model_backed":true,
                    "error":null,
                    "benchmark_category":"mcp"
                }]
            }"#,
        )
        .unwrap();
        let failures = live_evidence_failures(&passing_root, &args);
        assert!(failures.is_empty(), "{failures:?}");
        let verification = json_value_to_string(&live_evidence_verification_json(
            "live-evidence.json",
            &passing_root,
            &failures,
            false,
            &[],
        ));
        assert!(verification.contains("\"loop_surface_case_present\":true"));
    }

    #[test]
    fn repair_cache_evidence_summary_records_repair_and_cache_diagnostics() {
        let root = temp_test_dir("repair-cache-evidence-summary");
        fs::create_dir_all(&root).unwrap();
        let fixture = root.join("tool_repair_fixture.rs");
        fs::write(
            &fixture,
            "fn marker() { let _ = \"parse_tool_arguments_with_repair\"; }\n",
        )
        .unwrap();
        let runtime_root = root.join("runtime");
        let store = RuntimeStore::new(runtime_root.clone());

        let summary = repair_cache_evidence_summary_json(
            &store,
            &runtime_root,
            &root,
            "deepseek-v4-flash",
            fixture.to_str().expect("utf8 path"),
        )
        .unwrap();
        let json = json_value_to_string(&summary);

        assert!(json.contains("\"kind\":\"deepseek.dogfood.repair_cache_evidence.v1\""));
        assert!(json.contains("\"formerly_failing_trace_recovers\":true"));
        assert!(json.contains("\"every_repaired_call_observable\":true"));
        assert!(json.contains("\"cache_diagnostics_visible\":true"));
        assert!(json.contains("\"hit_rate_delta_basis_points\":7500"));

        let root_json = json_as_object(&summary).expect("summary object");
        let after_thread_id = root_json
            .get("after_thread_id")
            .and_then(json_as_string)
            .expect("after thread");
        let after_events = store.read_events(after_thread_id, 0).unwrap();
        assert_eq!(count_events(&after_events, "tool_call_repair"), 1);
        assert_eq!(count_events(&after_events, "prompt_layers_recorded"), 1);

        let commands = root_json
            .get("commands")
            .and_then(json_as_array)
            .expect("commands");
        assert!(commands
            .iter()
            .filter_map(json_as_string)
            .any(|command| command.contains("deepseek events replay")));
        assert!(commands
            .iter()
            .filter_map(json_as_string)
            .any(|command| command.contains("deepseek events diff")));
        assert!(commands
            .iter()
            .filter_map(json_as_string)
            .any(|command| command.contains("deepseek stats --thread")));

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn external_fixture_evidence_summary_records_release_ready_row() {
        let root = temp_test_dir("external-fixture-evidence-summary");
        let ledger = root.join("ledger.jsonl");
        let mut before_record = test_record(20, "write_validate", DogfoodOutcome::Success);
        before_record.model_transport = MODEL_TRANSPORT_ONLINE.to_string();
        let mut external_record = test_record(21, "write_validate", DogfoodOutcome::Success);
        external_record.model_transport = MODEL_TRANSPORT_ONLINE.to_string();
        external_record.notes = Some("external-write-fixture; disposable repo".to_string());
        append_record(&ledger, &before_record).unwrap();
        append_record(&ledger, &external_record).unwrap();
        let before_records = vec![before_record];
        let after_records = vec![before_records[0].clone(), external_record];
        let args = DogfoodExternalFixtureArgs {
            task: "replace `a - b` with `a + b` in src/lib.rs and validate with cargo test"
                .to_string(),
            workdir: "/tmp/disposable-repo".to_string(),
            budget: Some(12),
            benchmark_gate: true,
            evidence_out: Some("/tmp/external-fixture-evidence.json".to_string()),
            notes: Some("disposable repo".to_string()),
            dry_run: false,
            allow_offline: false,
        };

        let summary = external_fixture_evidence_summary_json(
            Path::new("/tmp/disposable-repo"),
            &ledger,
            &root.join("latest.md"),
            &args,
            MODEL_TRANSPORT_ONLINE,
            &before_records,
            &after_records,
            dogfood_file_fingerprint_json(&ledger),
            None,
            "cargo test",
        );
        let json = json_value_to_string(&summary);

        assert!(json.contains("\"kind\":\"deepseek.dogfood.external_fixture_evidence.v1\""));
        assert!(json.contains("\"release_evidence_ready\":true"));
        assert!(json.contains("\"post_validation_command\":\"cargo test\""));
        assert!(json.contains("\"post_validation_passed\":true"));
        assert!(json.contains("\"appended_records\":1"));
        assert!(json.contains("\"appended_model_backed_records\":1"));
        assert!(json.contains("\"appended_external_write_fixtures\":1"));
        assert!(json.contains("\"appended_successful_external_write_fixtures\":1"));
        assert!(json.contains("\"ledger_fingerprint\":{\"algorithm\":\"fnv1a64\""));
        assert!(json.contains("\"model_backed\":true"));
        assert!(json.contains("\"records\":[{\"benchmark_category\":\"write_validate\""));

        let out = root.join("external/evidence.json");
        write_external_fixture_evidence_summary(out.to_str().expect("utf8"), &summary).unwrap();
        let written = fs::read_to_string(&out).unwrap();
        assert!(written.contains("\"deepseek.dogfood.external_fixture_evidence.v1\""));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn live_run_api_key_file_sets_and_restores_env() {
        let root = temp_test_dir("api-key-file");
        fs::create_dir_all(&root).unwrap();
        let key_path = root.join("deepseek.key");
        fs::write(&key_path, "secret-from-file\n").unwrap();
        let env_name = format!(
            "DSCODE_DOGFOOD_KEY_FILE_TEST_{}_{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        );
        let mut config = crate::config::types::AppConfig::default();
        config.model.api_key_env = env_name.clone();
        unsafe {
            env::set_var(&env_name, "previous-value");
        }

        {
            let _guard =
                load_live_run_api_key_file(&config, key_path.to_str().expect("utf8")).unwrap();
            assert_eq!(env::var(&env_name).unwrap(), "secret-from-file");
        }

        assert_eq!(env::var(&env_name).unwrap(), "previous-value");
        unsafe {
            env::remove_var(&env_name);
        }
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn live_run_selection_applies_category_filter_and_total_limit() {
        let plan = LivePlan {
            ledger_path: PathBuf::from(".dscode/dogfood/ledger.jsonl"),
            manifest_path: PathBuf::from(".dscode/benchmarks.txt"),
            model_transport: MODEL_TRANSPORT_ONLINE.to_string(),
            target_live_runs: 100,
            target_live_success_rate: 90.0,
            live_runs: 0,
            live_success: 0,
            category_plans: vec![
                LiveCategoryPlan {
                    category: "write_validate".to_string(),
                    target_runs: 25,
                    target_success_rate: 90.0,
                    live_runs: 0,
                    live_success: 0,
                    needed_runs: 25,
                    replayable_cases: vec!["write-1".to_string(), "write-2".to_string()],
                    recommended_cases: vec!["write-1".to_string(), "write-2".to_string()],
                },
                LiveCategoryPlan {
                    category: "recovery".to_string(),
                    target_runs: 25,
                    target_success_rate: 90.0,
                    live_runs: 0,
                    live_success: 0,
                    needed_runs: 25,
                    replayable_cases: vec!["recover-1".to_string()],
                    recommended_cases: vec!["recover-1".to_string()],
                },
            ],
        };

        let all = select_live_run_cases(&plan, &[], 2);
        assert_eq!(
            all,
            vec![
                LiveRunCase {
                    category: "write_validate".to_string(),
                    name: "write-1".to_string(),
                },
                LiveRunCase {
                    category: "recovery".to_string(),
                    name: "recover-1".to_string(),
                },
            ]
        );

        let balanced_then_refilled = select_live_run_cases(&plan, &[], 3);
        assert_eq!(
            balanced_then_refilled,
            vec![
                LiveRunCase {
                    category: "write_validate".to_string(),
                    name: "write-1".to_string(),
                },
                LiveRunCase {
                    category: "recovery".to_string(),
                    name: "recover-1".to_string(),
                },
                LiveRunCase {
                    category: "write_validate".to_string(),
                    name: "write-2".to_string(),
                },
            ]
        );

        let recovery = select_live_run_cases(&plan, &["recovery".to_string()], 2);
        assert_eq!(
            recovery,
            vec![LiveRunCase {
                category: "recovery".to_string(),
                name: "recover-1".to_string(),
            }]
        );
    }

    #[test]
    fn infer_benchmark_category_keeps_read_only_tasks_out_of_planning() {
        let category = infer_benchmark_category(
            "inspect repository layout and summarize the main entrypoints",
            "todo_write -> dispatch_subagent -> list_files -> read_file",
            0,
            0,
            true,
            None,
        );
        assert_eq!(category, "read_only");
    }

    #[test]
    fn benchmark_case_category_corrects_stale_planning_label_for_read_only_task() {
        let record = DogfoodRecord {
            version: 1,
            timestamp_secs: 1,
            duration_ms: 10,
            task: "inspect repository layout and summarize the main entrypoints".to_string(),
            skill: None,
            budget: 4,
            model: "x".to_string(),
            model_transport: MODEL_TRANSPORT_UNKNOWN.to_string(),
            workdir: ".".to_string(),
            outcome: DogfoodOutcome::Success,
            manual_intervention: false,
            notes: None,
            tool_calls: 4,
            failed_tool_calls: 0,
            repeated_call_failures: 0,
            diagnostic_expected_failure: false,
            used_subagent: true,
            final_message: "ok".to_string(),
            tool_trace: "todo_write -> dispatch_subagent -> list_files -> read_file".to_string(),
            error_kind: None,
            benchmark_category: Some("planning".to_string()),
            benchmark_seed_observations: None,
        };
        assert_eq!(benchmark_case_category(&record), "read_only");
    }

    #[test]
    fn benchmark_case_category_corrects_stale_read_only_label_for_natural_recovery_task() {
        let record = DogfoodRecord {
            version: 1,
            timestamp_secs: 1,
            duration_ms: 10,
            task: "find where `missing_fixture_symbol_js_20260509` is implemented, and if there are no matches inspect the repository layout instead".to_string(),
            skill: None,
            budget: 6,
            model: "x".to_string(),
            model_transport: MODEL_TRANSPORT_UNKNOWN.to_string(),
            workdir: ".".to_string(),
            outcome: DogfoodOutcome::Success,
            manual_intervention: false,
            notes: None,
            tool_calls: 4,
            failed_tool_calls: 0,
            repeated_call_failures: 0,
            diagnostic_expected_failure: false,
            used_subagent: false,
            final_message: "ok".to_string(),
            tool_trace: "todo_write -> search_text -> list_files -> read_file".to_string(),
            error_kind: None,
            benchmark_category: Some("read_only".to_string()),
            benchmark_seed_observations: None,
        };
        assert_eq!(benchmark_case_category(&record), "recovery");
    }

    #[test]
    fn benchmark_case_category_corrects_stale_write_validate_label_for_failure_repro_task() {
        let record = DogfoodRecord {
            version: 1,
            timestamp_secs: 1,
            duration_ms: 10,
            task: "investigate why npm test fails in the JavaScript CLI and inspect the failing test file before retrying".to_string(),
            skill: Some("debug".to_string()),
            budget: 4,
            model: "x".to_string(),
            model_transport: MODEL_TRANSPORT_UNKNOWN.to_string(),
            workdir: ".".to_string(),
            outcome: DogfoodOutcome::Failed,
            manual_intervention: false,
            notes: None,
            tool_calls: 2,
            failed_tool_calls: 1,
            repeated_call_failures: 0,
            diagnostic_expected_failure: false,
            used_subagent: false,
            final_message: "stopped after readback".to_string(),
            tool_trace: "run_shell -> read_file".to_string(),
            error_kind: Some("tool_failure".to_string()),
            benchmark_category: Some("write_validate".to_string()),
            benchmark_seed_observations: None,
        };
        assert_eq!(benchmark_case_category(&record), "recovery");
    }

    #[test]
    fn infer_benchmark_category_keeps_plan_only_tasks_as_planning() {
        let category = infer_benchmark_category(
            "plan an end-to-end improvement for benchmark reliability and report the execution steps before acting",
            "todo_write",
            0,
            0,
            false,
            None,
        );
        assert_eq!(category, "planning");
    }

    #[test]
    fn infer_benchmark_category_prefers_pr_workflow_for_pull_request_tasks() {
        let category = infer_benchmark_category(
            "Review pull request #42 and fix the failed CI job",
            "run_shell -> apply_patch",
            1,
            0,
            false,
            None,
        );
        assert_eq!(category, "pr_workflow");
    }

    #[test]
    fn infer_benchmark_category_marks_natural_search_fallback_as_recovery() {
        let category = infer_benchmark_category(
            "find where `missing_fixture_symbol_js_20260509` is implemented, and if there are no matches inspect the repository layout instead",
            "todo_write -> search_text -> list_files -> read_file",
            0,
            0,
            false,
            None,
        );
        assert_eq!(category, "recovery");
    }

    #[test]
    fn infer_benchmark_category_keeps_recovered_edit_retry_as_write_validate() {
        let category = infer_benchmark_category(
            "replace `a - b` with `a * b` in src/math_ops.py and validate with pytest until the tests pass",
            "apply_patch -> git_diff -> run_shell -> read_file -> apply_patch -> git_diff -> run_shell",
            1,
            0,
            false,
            None,
        );
        assert_eq!(category, "write_validate");
    }

    #[test]
    fn from_result_derives_metrics_from_tool_events() {
        let result = RunResult {
            final_message: "done".to_string(),
            tool_events: vec![
                ToolEvent {
                    tool_name: "todo_write".to_string(),
                    input: BTreeMap::new(),
                    output: "ok".to_string(),
                    status: ObservationStatus::Ok,
                },
                ToolEvent {
                    tool_name: "dispatch_subagent".to_string(),
                    input: BTreeMap::new(),
                    output: "repeated identical tool call detected".to_string(),
                    status: ObservationStatus::Failed,
                },
            ],
            usage: TokenUsage::default(),
            prompt_layers: Vec::new(),
        };
        let args = DogfoodRunArgs {
            task: "debug parser".to_string(),
            from_benchmark: None,
            benchmark_manifest: None,
            skill: None,
            budget: Some(4),
            workdir: None,
            isolate_workdir: false,
            outcome: None,
            manual_intervention: false,
            benchmark_gate: false,
            notes: None,
        };
        let record = DogfoodRecord::from_result(
            1,
            10,
            "deepseek-v4-pro".to_string(),
            MODEL_TRANSPORT_UNKNOWN,
            ".".to_string(),
            4,
            &args,
            false,
            &result,
        );
        assert!(matches!(record.outcome, DogfoodOutcome::Stuck));
        assert!(!record.diagnostic_expected_failure);
        assert!(record.used_subagent);
        assert_eq!(record.failed_tool_calls, 1);
        assert!(record
            .benchmark_seed_observations
            .as_deref()
            .unwrap_or("")
            .contains("dispatch_subagent:failed"));
    }

    #[test]
    fn from_result_treats_empty_no_tool_response_as_failed() {
        let result = RunResult {
            final_message: "DeepSeek returned no content.".to_string(),
            tool_events: Vec::new(),
            usage: TokenUsage::default(),
            prompt_layers: Vec::new(),
        };
        let args = DogfoodRunArgs {
            task: "replace `a - b` with `a + b` in src/lib.rs and validate with cargo test"
                .to_string(),
            from_benchmark: None,
            benchmark_manifest: None,
            skill: None,
            budget: Some(4),
            workdir: None,
            isolate_workdir: false,
            outcome: None,
            manual_intervention: false,
            benchmark_gate: false,
            notes: None,
        };
        let record = DogfoodRecord::from_result(
            1,
            10,
            "deepseek-v4-flash".to_string(),
            MODEL_TRANSPORT_UNKNOWN,
            ".".to_string(),
            4,
            &args,
            false,
            &result,
        );
        assert_eq!(record.tool_calls, 0);
        assert_eq!(record.failed_tool_calls, 0);
        assert!(matches!(record.outcome, DogfoodOutcome::Failed));
    }

    #[test]
    fn from_result_treats_meta_result_failed_as_failed_outcome() {
        let result = RunResult {
            final_message: "read back the failing file".to_string(),
            tool_events: vec![
                ToolEvent {
                    tool_name: "run_shell".to_string(),
                    input: BTreeMap::new(),
                    output: "meta.command_kind=test\nmeta.exit_code=101\nmeta.result=failed\nmeta.failure_kind=test_failure".to_string(),
                    status: ObservationStatus::Ok,
                },
                ToolEvent {
                    tool_name: "read_file".to_string(),
                    input: BTreeMap::new(),
                    output: "1 pub fn add(a: i32, b: i32) -> i32 {".to_string(),
                    status: ObservationStatus::Ok,
                },
            ],
            usage: TokenUsage::default(),
            prompt_layers: Vec::new(),
        };
        let args = DogfoodRunArgs {
            task: "replace `a - b` with `a * b` in src/lib.rs and validate with cargo test"
                .to_string(),
            from_benchmark: None,
            benchmark_manifest: None,
            skill: None,
            budget: Some(6),
            workdir: None,
            isolate_workdir: false,
            outcome: None,
            manual_intervention: false,
            benchmark_gate: false,
            notes: None,
        };
        let record = DogfoodRecord::from_result(
            1,
            10,
            "deepseek-v4-pro".to_string(),
            MODEL_TRANSPORT_UNKNOWN,
            ".".to_string(),
            6,
            &args,
            false,
            &result,
        );
        assert_eq!(record.failed_tool_calls, 1);
        assert!(matches!(record.outcome, DogfoodOutcome::Failed));
        assert!(!record.diagnostic_expected_failure);
    }

    #[test]
    fn from_result_treats_recovered_validation_retry_as_success() {
        let result = RunResult {
            final_message: "tests pass".to_string(),
            tool_events: vec![
                ToolEvent {
                    tool_name: "apply_patch".to_string(),
                    input: BTreeMap::new(),
                    output: "Updated src/math_ops.py using single replacement mode.".to_string(),
                    status: ObservationStatus::Ok,
                },
                ToolEvent {
                    tool_name: "git_diff".to_string(),
                    input: BTreeMap::new(),
                    output: "No local diff.".to_string(),
                    status: ObservationStatus::Ok,
                },
                ToolEvent {
                    tool_name: "run_shell".to_string(),
                    input: BTreeMap::new(),
                    output: "meta.command_kind=test\nmeta.exit_code=1\nmeta.result=failed\nmeta.failure_kind=test_failure".to_string(),
                    status: ObservationStatus::Ok,
                },
                ToolEvent {
                    tool_name: "read_file".to_string(),
                    input: BTreeMap::new(),
                    output: "2     return a * b".to_string(),
                    status: ObservationStatus::Ok,
                },
                ToolEvent {
                    tool_name: "apply_patch".to_string(),
                    input: BTreeMap::new(),
                    output: "Updated src/math_ops.py using single replacement mode.".to_string(),
                    status: ObservationStatus::Ok,
                },
                ToolEvent {
                    tool_name: "git_diff".to_string(),
                    input: BTreeMap::new(),
                    output: "No local diff.".to_string(),
                    status: ObservationStatus::Ok,
                },
                ToolEvent {
                    tool_name: "run_shell".to_string(),
                    input: BTreeMap::new(),
                    output: "meta.command_kind=test\nmeta.exit_code=0\nmeta.result=ok".to_string(),
                    status: ObservationStatus::Ok,
                },
            ],
            usage: TokenUsage::default(),
            prompt_layers: Vec::new(),
        };
        let args = DogfoodRunArgs {
            task: "replace `a - b` with `a * b` in src/math_ops.py and validate with pytest until the tests pass"
                .to_string(),
            from_benchmark: None,
            benchmark_manifest: None,
            skill: None,
            budget: Some(8),
            workdir: None,
            isolate_workdir: false,
            outcome: None,
            manual_intervention: false,
            benchmark_gate: false,
            notes: None,
        };
        let record = DogfoodRecord::from_result(
            1,
            10,
            "deepseek-v4-pro".to_string(),
            MODEL_TRANSPORT_UNKNOWN,
            ".".to_string(),
            8,
            &args,
            false,
            &result,
        );
        assert_eq!(record.failed_tool_calls, 1);
        assert!(matches!(record.outcome, DogfoodOutcome::Success));
        assert_eq!(record.benchmark_category.as_deref(), Some("write_validate"));
    }

    #[test]
    fn from_result_treats_validated_patch_as_success_after_repeated_read_recovery() {
        let result = RunResult {
            final_message: "tests pass".to_string(),
            tool_events: vec![
                ToolEvent {
                    tool_name: "read_file".to_string(),
                    input: BTreeMap::new(),
                    output: "1 pub fn add(a: i32, b: i32) -> i32 {".to_string(),
                    status: ObservationStatus::Ok,
                },
                ToolEvent {
                    tool_name: "read_file".to_string(),
                    input: BTreeMap::new(),
                    output: "repeated identical tool call detected".to_string(),
                    status: ObservationStatus::Failed,
                },
                ToolEvent {
                    tool_name: "apply_patch".to_string(),
                    input: BTreeMap::new(),
                    output: "Updated src/lib.rs using single replacement mode.".to_string(),
                    status: ObservationStatus::Ok,
                },
                ToolEvent {
                    tool_name: "run_shell".to_string(),
                    input: BTreeMap::new(),
                    output: "meta.command_kind=test\nmeta.exit_code=0\nmeta.result=ok".to_string(),
                    status: ObservationStatus::Ok,
                },
            ],
            usage: TokenUsage::default(),
            prompt_layers: Vec::new(),
        };
        let args = DogfoodRunArgs {
            task: "replace `a - b` with `a + b` in src/lib.rs and validate with cargo test"
                .to_string(),
            from_benchmark: None,
            benchmark_manifest: None,
            skill: None,
            budget: Some(8),
            workdir: None,
            isolate_workdir: false,
            outcome: None,
            manual_intervention: false,
            benchmark_gate: false,
            notes: None,
        };
        let record = DogfoodRecord::from_result(
            1,
            10,
            "deepseek-chat".to_string(),
            MODEL_TRANSPORT_UNKNOWN,
            ".".to_string(),
            8,
            &args,
            false,
            &result,
        );
        assert_eq!(record.failed_tool_calls, 1);
        assert_eq!(record.repeated_call_failures, 1);
        assert!(matches!(record.outcome, DogfoodOutcome::Success));
    }

    #[test]
    fn from_result_treats_expected_failure_diagnosis_as_success() {
        let result = RunResult {
            final_message: "read back the failing test file".to_string(),
            tool_events: vec![
                ToolEvent {
                    tool_name: "run_shell".to_string(),
                    input: BTreeMap::new(),
                    output: "meta.command_kind=test\nmeta.exit_code=101\nmeta.result=failed\nmeta.failure_kind=test_failure".to_string(),
                    status: ObservationStatus::Ok,
                },
                ToolEvent {
                    tool_name: "read_file".to_string(),
                    input: BTreeMap::new(),
                    output: "test('route benchmark stays stable', () => {})".to_string(),
                    status: ObservationStatus::Ok,
                },
            ],
            usage: TokenUsage::default(),
            prompt_layers: Vec::new(),
        };
        let args = DogfoodRunArgs {
            task: "investigate why npm test fails in the JavaScript CLI and inspect the failing test file before retrying"
                .to_string(),
            from_benchmark: None,
            benchmark_manifest: None,
            skill: Some("debug".to_string()),
            budget: Some(4),
            workdir: None,
            isolate_workdir: false,
            outcome: None,
            manual_intervention: false,
            benchmark_gate: false,
            notes: None,
        };
        let record = DogfoodRecord::from_result(
            1,
            10,
            "deepseek-v4-pro".to_string(),
            MODEL_TRANSPORT_UNKNOWN,
            ".".to_string(),
            4,
            &args,
            false,
            &result,
        );
        assert_eq!(record.failed_tool_calls, 1);
        assert!(matches!(record.outcome, DogfoodOutcome::Success));
        assert!(record.diagnostic_expected_failure);
    }

    #[test]
    fn prepare_run_workdir_clones_fixture_when_isolation_enabled() {
        let fixture_root =
            std::env::temp_dir().join(format!("deepseek-dogfood-fixture-{}", std::process::id()));
        let fixture = fixture_root.join("fixture");
        let nested = fixture.join("src");
        fs::create_dir_all(&nested).unwrap();
        fs::write(
            nested.join("lib.rs"),
            "pub fn add(a: i32, b: i32) -> i32 { a - b }\n",
        )
        .unwrap();

        let (execution, cleanup) = prepare_run_workdir(&fixture, true).unwrap();
        assert_ne!(execution, fixture);
        assert!(execution.join("src/lib.rs").is_file());

        fs::remove_dir_all(fixture_root).ok();
        if let Some(path) = cleanup {
            fs::remove_dir_all(path).ok();
        }
    }

    #[test]
    fn resolve_run_args_loads_task_defaults_from_benchmark_case() {
        let root = std::env::temp_dir().join(format!(
            "deepseek-dogfood-benchmark-run-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let manifest_path = root.join("benchmarks.txt");
        fs::write(
            &manifest_path,
            r#"name = "fixture-pr-retry-validate-rust-mini"
task = "Address PR #48 review feedback: replace `a - b` with `a * b` in src/lib.rs and validate with cargo test until the tests pass."
category = "pr_workflow"
skill = "verify-changes"
workdir = "fixtures/rust-write-mini"
isolate_workdir = true
budget = 8
notes = "Real PR workflow retry case over an isolated Rust fixture"
"#,
        )
        .unwrap();

        let args = DogfoodRunArgs {
            task: String::new(),
            from_benchmark: Some("fixture-pr-retry-validate-rust-mini".to_string()),
            benchmark_manifest: Some(manifest_path.display().to_string()),
            skill: None,
            budget: None,
            workdir: None,
            isolate_workdir: false,
            outcome: None,
            manual_intervention: false,
            benchmark_gate: false,
            notes: None,
        };
        let resolved = resolve_run_args(&crate::config::types::AppConfig::default(), args).unwrap();

        assert!(resolved
            .task
            .contains("replace `a - b` with `a * b` in src/lib.rs"));
        assert_eq!(resolved.skill.as_deref(), Some("verify-changes"));
        assert_eq!(resolved.budget, Some(8));
        let expected_workdir = root.join("fixtures/rust-write-mini").display().to_string();
        assert_eq!(resolved.workdir.as_deref(), Some(expected_workdir.as_str()));
        assert!(resolved.isolate_workdir);
        assert_eq!(
            resolved.notes.as_deref(),
            Some("Real PR workflow retry case over an isolated Rust fixture")
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn benchmark_replay_auto_approve_env_is_temporary() {
        unsafe {
            env::remove_var("DSCODE_AUTO_APPROVE_WRITES");
            env::remove_var("DSCODE_AUTO_APPROVE_SHELL");
            env::remove_var("DSCODE_AUTO_APPROVE_MCP");
        }

        let root = temp_test_dir("dogfood-auto-approve");
        fs::create_dir_all(&root).unwrap();

        let snapshot = run_task_in_workdir(&root, &root, true, || {
            Ok((
                env::var("DSCODE_AUTO_APPROVE_WRITES").ok(),
                env::var("DSCODE_AUTO_APPROVE_SHELL").ok(),
                env::var("DSCODE_AUTO_APPROVE_MCP").ok(),
            ))
        })
        .unwrap();

        assert_eq!(snapshot.0.as_deref(), Some("1"));
        assert_eq!(snapshot.1.as_deref(), Some("1"));
        assert_eq!(snapshot.2.as_deref(), Some("1"));
        assert!(env::var("DSCODE_AUTO_APPROVE_WRITES").is_err());
        assert!(env::var("DSCODE_AUTO_APPROVE_SHELL").is_err());
        assert!(env::var("DSCODE_AUTO_APPROVE_MCP").is_err());

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn resolve_run_args_keeps_explicit_overrides_over_benchmark_defaults() {
        let root = std::env::temp_dir().join(format!(
            "deepseek-dogfood-benchmark-override-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let manifest_path = root.join("benchmarks.txt");
        fs::write(
            &manifest_path,
            r#"name = "fixture-inspect-rust-cli-mini"
task = "inspect the fixture"
category = "read_only"
skill = "research"
workdir = "fixtures/rust-cli-mini"
budget = 6
"#,
        )
        .unwrap();

        let args = DogfoodRunArgs {
            task: String::new(),
            from_benchmark: Some("fixture-inspect-rust-cli-mini".to_string()),
            benchmark_manifest: Some(manifest_path.display().to_string()),
            skill: Some("debug".to_string()),
            budget: Some(3),
            workdir: Some("custom-fixture".to_string()),
            isolate_workdir: true,
            outcome: None,
            manual_intervention: false,
            benchmark_gate: false,
            notes: Some("custom note".to_string()),
        };
        let resolved = resolve_run_args(&crate::config::types::AppConfig::default(), args).unwrap();

        assert_eq!(resolved.skill.as_deref(), Some("debug"));
        assert_eq!(resolved.budget, Some(3));
        assert_eq!(resolved.workdir.as_deref(), Some("custom-fixture"));
        assert!(resolved.isolate_workdir);
        assert_eq!(resolved.notes.as_deref(), Some("custom note"));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn select_replayable_cases_skips_seed_only_cases() {
        let cases = vec![
            BenchmarkCaseSummary {
                name: "seeded-pr-review".to_string(),
                task: "review pull request".to_string(),
                category: "pr_workflow".to_string(),
                skill: None,
                workdir: Some("fixtures/rust-cli-mini".to_string()),
                isolate_workdir: false,
                budget: 4,
                notes: None,
                seed_observations: Some("git_diff:ok:src/lib.rs".to_string()),
            },
            BenchmarkCaseSummary {
                name: "fixture-pr-retry-validate-rust-mini".to_string(),
                task: "fix and validate".to_string(),
                category: "pr_workflow".to_string(),
                skill: None,
                workdir: Some("fixtures/rust-write-mini".to_string()),
                isolate_workdir: true,
                budget: 8,
                notes: None,
                seed_observations: None,
            },
        ];

        let selected = select_replayable_cases(&cases, Some("pr_workflow"), None);
        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].name, "fixture-pr-retry-validate-rust-mini");
    }

    #[test]
    fn select_replayable_cases_skips_expected_validation_failure_readback_cases() {
        let cases = vec![
            BenchmarkCaseSummary {
                name: "fixture-recover-write-validate-rust-mini".to_string(),
                task: "replace `a - b` with `a * b` in src/lib.rs and validate with cargo test"
                    .to_string(),
                category: "write_validate".to_string(),
                skill: None,
                workdir: Some("fixtures/rust-write-mini".to_string()),
                isolate_workdir: true,
                budget: 6,
                notes: Some(
                    "Real isolated Rust write+validate failure case for apply_patch -> git_diff -> run_shell -> read_file"
                        .to_string(),
                ),
                seed_observations: None,
            },
            BenchmarkCaseSummary {
                name: "fixture-retry-write-validate-rust-mini".to_string(),
                task: "replace `a - b` with `a * b` in src/lib.rs and validate with cargo test until the tests pass"
                    .to_string(),
                category: "write_validate".to_string(),
                skill: None,
                workdir: Some("fixtures/rust-write-mini".to_string()),
                isolate_workdir: true,
                budget: 8,
                notes: Some("Real isolated Rust write+validate retry case".to_string()),
                seed_observations: None,
            },
        ];

        let selected = select_replayable_cases(&cases, Some("write_validate"), None);
        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].name, "fixture-retry-write-validate-rust-mini");
    }

    #[test]
    fn select_replayable_cases_applies_category_and_limit() {
        let cases = vec![
            BenchmarkCaseSummary {
                name: "fixture-a".to_string(),
                task: "a".to_string(),
                category: "write_validate".to_string(),
                skill: None,
                workdir: Some("fixtures/a".to_string()),
                isolate_workdir: true,
                budget: 6,
                notes: None,
                seed_observations: None,
            },
            BenchmarkCaseSummary {
                name: "fixture-b".to_string(),
                task: "b".to_string(),
                category: "write_validate".to_string(),
                skill: None,
                workdir: Some("fixtures/b".to_string()),
                isolate_workdir: true,
                budget: 6,
                notes: None,
                seed_observations: None,
            },
            BenchmarkCaseSummary {
                name: "fixture-c".to_string(),
                task: "c".to_string(),
                category: "recovery".to_string(),
                skill: None,
                workdir: Some("fixtures/c".to_string()),
                isolate_workdir: true,
                budget: 6,
                notes: None,
                seed_observations: None,
            },
        ];

        let selected = select_replayable_cases(&cases, Some("write_validate"), Some(1));
        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].name, "fixture-a");
    }

    #[test]
    fn render_benchmark_seed_export_emits_non_success_records() {
        let records = vec![
            DogfoodRecord {
                version: 1,
                timestamp_secs: 10,
                duration_ms: 4,
                task: "investigate repeated list_files loop".to_string(),
                skill: None,
                budget: 6,
                model: "x".to_string(),
                model_transport: MODEL_TRANSPORT_UNKNOWN.to_string(),
                workdir: "/repo".to_string(),
                outcome: DogfoodOutcome::Stuck,
                manual_intervention: false,
                notes: Some("dogfood seed".to_string()),
                tool_calls: 3,
                failed_tool_calls: 1,
                repeated_call_failures: 1,
                diagnostic_expected_failure: false,
                used_subagent: false,
                final_message: "stuck".to_string(),
                tool_trace: "list_files -> list_files".to_string(),
                error_kind: None,
                benchmark_category: Some("recovery".to_string()),
                benchmark_seed_observations: Some(
                    "list_files:failed:repeated identical tool call detected".to_string(),
                ),
            },
            DogfoodRecord {
                version: 1,
                timestamp_secs: 11,
                duration_ms: 3,
                task: "inspect repository".to_string(),
                skill: None,
                budget: 4,
                model: "x".to_string(),
                model_transport: MODEL_TRANSPORT_UNKNOWN.to_string(),
                workdir: "/repo".to_string(),
                outcome: DogfoodOutcome::Success,
                manual_intervention: false,
                notes: None,
                tool_calls: 2,
                failed_tool_calls: 0,
                repeated_call_failures: 0,
                diagnostic_expected_failure: false,
                used_subagent: false,
                final_message: "ok".to_string(),
                tool_trace: "list_files -> read_file".to_string(),
                error_kind: None,
                benchmark_category: Some("read_only".to_string()),
                benchmark_seed_observations: Some("list_files:ok:src/".to_string()),
            },
        ];

        let export = render_benchmark_seed_export(&records, 10, None, Path::new("/repo"));
        assert!(export.contains("name = \"dogfood-stuck-"));
        assert!(export.contains("task = \"investigate repeated list_files loop\""));
        assert!(export.contains("category = \"recovery\""));
        assert!(export.contains(
            "seed_observations = \"list_files:failed:repeated identical tool call detected\""
        ));
        assert!(!export.contains("inspect repository"));
    }

    #[test]
    fn build_promotion_plan_skips_duplicate_cases_and_renames_conflicts() {
        let records = vec![
            DogfoodRecord {
                version: 1,
                timestamp_secs: 10,
                duration_ms: 4,
                task: "investigate repeated list_files loop".to_string(),
                skill: None,
                budget: 6,
                model: "x".to_string(),
                model_transport: MODEL_TRANSPORT_UNKNOWN.to_string(),
                workdir: "/repo".to_string(),
                outcome: DogfoodOutcome::Stuck,
                manual_intervention: false,
                notes: Some("dogfood seed".to_string()),
                tool_calls: 3,
                failed_tool_calls: 1,
                repeated_call_failures: 1,
                diagnostic_expected_failure: false,
                used_subagent: false,
                final_message: "stuck".to_string(),
                tool_trace: "list_files -> list_files".to_string(),
                error_kind: None,
                benchmark_category: Some("recovery".to_string()),
                benchmark_seed_observations: Some(
                    "list_files:failed:repeated identical tool call detected".to_string(),
                ),
            },
            DogfoodRecord {
                version: 1,
                timestamp_secs: 11,
                duration_ms: 4,
                task: "investigate repeated list_files loop".to_string(),
                skill: None,
                budget: 6,
                model: "x".to_string(),
                model_transport: MODEL_TRANSPORT_UNKNOWN.to_string(),
                workdir: "/repo".to_string(),
                outcome: DogfoodOutcome::Stuck,
                manual_intervention: false,
                notes: Some("second seed".to_string()),
                tool_calls: 3,
                failed_tool_calls: 1,
                repeated_call_failures: 1,
                diagnostic_expected_failure: false,
                used_subagent: false,
                final_message: "stuck again".to_string(),
                tool_trace: "list_files -> search_text".to_string(),
                error_kind: None,
                benchmark_category: Some("recovery".to_string()),
                benchmark_seed_observations: Some(
                    "search_text:failed:no matches || recovery_hint:ok:after=search_text; next=list_files".to_string(),
                ),
            },
        ];
        let existing = vec![BenchmarkCaseSummary {
            name: "dogfood-stuck-investigate-repeated-list-files".to_string(),
            task: "investigate repeated list_files loop".to_string(),
            category: "recovery".to_string(),
            skill: None,
            workdir: None,
            isolate_workdir: false,
            budget: 4,
            notes: None,
            seed_observations: Some(
                "list_files:failed:repeated identical tool call detected".to_string(),
            ),
        }];

        let plan = build_promotion_plan(&records, &existing, 10, None, Path::new("/repo"));
        assert_eq!(plan.duplicates_skipped, 1);
        assert_eq!(plan.policy_skipped, 0);
        assert!(plan.policy_skip_reasons.is_empty());
        assert_eq!(plan.cases.len(), 1);
        assert_eq!(
            plan.cases[0].name,
            "dogfood-stuck-investigate-repeated-list-files-2"
        );
        assert!(plan.cases[0].block.contains("category = \"recovery\""));
        assert!(plan.cases[0]
            .block
            .contains("seed_observations = \"search_text:failed:no matches"));
    }

    #[test]
    fn build_promotion_plan_skips_manual_and_long_trace_by_default_policy() {
        let records = vec![
            DogfoodRecord {
                version: 1,
                timestamp_secs: 10,
                duration_ms: 4,
                task: "manual triage".to_string(),
                skill: None,
                budget: 6,
                model: "x".to_string(),
                model_transport: MODEL_TRANSPORT_UNKNOWN.to_string(),
                workdir: "/repo".to_string(),
                outcome: DogfoodOutcome::Manual,
                manual_intervention: true,
                notes: None,
                tool_calls: 4,
                failed_tool_calls: 1,
                repeated_call_failures: 0,
                diagnostic_expected_failure: false,
                used_subagent: false,
                final_message: "manual".to_string(),
                tool_trace: "search_text -> read_file -> list_files -> todo_write".to_string(),
                error_kind: None,
                benchmark_category: Some("planning".to_string()),
                benchmark_seed_observations: Some("search_text:failed:no matches".to_string()),
            },
            DogfoodRecord {
                version: 1,
                timestamp_secs: 11,
                duration_ms: 4,
                task: "too many hops".to_string(),
                skill: None,
                budget: 12,
                model: "x".to_string(),
                model_transport: MODEL_TRANSPORT_UNKNOWN.to_string(),
                workdir: "/repo".to_string(),
                outcome: DogfoodOutcome::Failed,
                manual_intervention: false,
                notes: None,
                tool_calls: 9,
                failed_tool_calls: 1,
                repeated_call_failures: 0,
                diagnostic_expected_failure: false,
                used_subagent: false,
                final_message: "failed".to_string(),
                tool_trace: "a -> b -> c -> d -> e -> f -> g -> h -> i".to_string(),
                error_kind: None,
                benchmark_category: Some("read_only".to_string()),
                benchmark_seed_observations: Some("run_shell:failed:boom".to_string()),
            },
        ];

        let plan = build_promotion_plan(&records, &[], 10, None, Path::new("/repo"));
        assert_eq!(plan.cases.len(), 0);
        assert_eq!(plan.policy_skipped, 2);
        assert_eq!(plan.policy_skip_reasons.len(), 2);
        assert!(plan.policy_skip_reasons.iter().any(|reason| {
            reason.reason_code == "manual_requires_explicit_filter"
                && reason.count == 1
                && reason.example_task == "manual triage"
        }));
        assert!(plan.policy_skip_reasons.iter().any(|reason| {
            reason.reason_code == "tool_trace_too_long"
                && reason.count == 1
                && reason.example_task == "too many hops"
        }));
    }

    #[test]
    fn build_promotion_plan_allows_manual_when_explicitly_filtered() {
        let records = vec![DogfoodRecord {
            version: 1,
            timestamp_secs: 10,
            duration_ms: 4,
            task: "manual triage".to_string(),
            skill: None,
            budget: 6,
            model: "x".to_string(),
            model_transport: MODEL_TRANSPORT_UNKNOWN.to_string(),
            workdir: "/repo".to_string(),
            outcome: DogfoodOutcome::Manual,
            manual_intervention: true,
            notes: None,
            tool_calls: 4,
            failed_tool_calls: 1,
            repeated_call_failures: 0,
            diagnostic_expected_failure: false,
            used_subagent: false,
            final_message: "manual".to_string(),
            tool_trace: "search_text -> read_file -> list_files -> todo_write".to_string(),
            error_kind: None,
            benchmark_category: Some("planning".to_string()),
            benchmark_seed_observations: Some("search_text:failed:no matches".to_string()),
        }];

        let plan = build_promotion_plan(
            &records,
            &[],
            10,
            Some(DogfoodOutcome::Manual),
            Path::new("/repo"),
        );
        assert_eq!(plan.cases.len(), 1);
        assert_eq!(plan.policy_skipped, 0);
        assert!(plan.policy_skip_reasons.is_empty());
    }

    #[test]
    fn build_promotion_plan_reports_missing_failure_signal_reason() {
        let records = vec![DogfoodRecord {
            version: 1,
            timestamp_secs: 10,
            duration_ms: 4,
            task: "ambiguous success-looking run".to_string(),
            skill: None,
            budget: 6,
            model: "x".to_string(),
            model_transport: MODEL_TRANSPORT_UNKNOWN.to_string(),
            workdir: "/repo".to_string(),
            outcome: DogfoodOutcome::Failed,
            manual_intervention: false,
            notes: None,
            tool_calls: 3,
            failed_tool_calls: 0,
            repeated_call_failures: 0,
            diagnostic_expected_failure: false,
            used_subagent: false,
            final_message: "gave up without a hard failure".to_string(),
            tool_trace: "search_text -> read_file -> todo_write".to_string(),
            error_kind: None,
            benchmark_category: Some("planning".to_string()),
            benchmark_seed_observations: Some("search_text:ok:hit".to_string()),
        }];

        let plan = build_promotion_plan(&records, &[], 10, None, Path::new("/repo"));
        assert_eq!(plan.cases.len(), 0);
        assert_eq!(plan.policy_skipped, 1);
        assert_eq!(
            plan.policy_skip_reasons,
            vec![PolicySkipReasonCount {
                reason_code: "missing_failure_signal",
                reason_label: "missing failed/stuck/manual signal",
                count: 1,
                example_task: "ambiguous success-looking run".to_string(),
            }]
        );
    }

    #[test]
    fn append_promoted_cases_separates_blocks() {
        let root = std::env::temp_dir().join(format!("deepseek-promote-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let manifest_path = root.join("benchmarks.txt");
        fs::write(
            &manifest_path,
            "name = \"existing\"\ntask = \"inspect repo\"\n",
        )
        .unwrap();
        let cases = vec![
            PromotedBenchmarkCase {
                name: "case-one".to_string(),
                block: "name = \"case-one\"\ntask = \"one\"\n".to_string(),
            },
            PromotedBenchmarkCase {
                name: "case-two".to_string(),
                block: "name = \"case-two\"\ntask = \"two\"\n".to_string(),
            },
        ];

        append_promoted_cases(&manifest_path, &cases).unwrap();
        let written = fs::read_to_string(&manifest_path).unwrap();
        assert!(
            written.contains("name = \"existing\"\ntask = \"inspect repo\"\n\nname = \"case-one\"")
        );
        assert!(written.contains("name = \"case-one\"\ntask = \"one\"\n\nname = \"case-two\""));

        let _ = fs::remove_dir_all(root);
    }
}
