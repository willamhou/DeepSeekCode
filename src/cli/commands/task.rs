use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

#[cfg(unix)]
use std::os::unix::process::CommandExt;

use crate::cli::app::{
    TaskAction, TaskDiffArgs, TaskFixtureSmokeArgs, TaskListArgs, TaskMergeArgs, TaskRejectArgs,
    TaskShowArgs, TaskStartArgs, TaskStopArgs,
};
use crate::error::{app_error, AppResult};
use crate::util::json::{json_value_to_string, parse_root_object, JsonValue};

const TASK_SCHEMA: &str = "deepseek.task_runner.task.v1";
const SMOKE_SCHEMA: &str = "deepseek.task_runner.fixture_smoke.v1";

#[derive(Debug, Clone, PartialEq, Eq)]
struct TaskRecord {
    id: String,
    task: String,
    status: String,
    repo_root: PathBuf,
    worktree: PathBuf,
    branch: String,
    base_ref: String,
    pid: Option<u32>,
    exit_code: Option<i32>,
    stdout_log: PathBuf,
    stderr_log: PathBuf,
    created_at: u64,
    updated_at: u64,
    skill: Option<String>,
    budget: Option<usize>,
    command: Vec<String>,
}

#[derive(Debug, Clone)]
struct TaskPaths {
    repo_root: PathBuf,
    records_dir: PathBuf,
    logs_dir: PathBuf,
    worktrees_dir: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct MergeResult {
    patch_bytes: usize,
    untracked_files: usize,
}

pub fn run(action: TaskAction) -> AppResult<()> {
    match action {
        TaskAction::Start(args) => run_start(args),
        TaskAction::List(args) => run_list(args),
        TaskAction::Show(args) => run_show(args),
        TaskAction::Stop(args) => run_stop(args),
        TaskAction::Diff(args) => run_diff(args),
        TaskAction::Merge(args) => run_merge(args),
        TaskAction::Reject(args) => run_reject(args),
        TaskAction::FixtureSmoke(args) => run_fixture_smoke(args),
    }
}

fn run_start(args: TaskStartArgs) -> AppResult<()> {
    let json = args.json;
    if json {
        println!("{}", start_task_json(args)?);
        return Ok(());
    }
    let record = start_task(args)?;
    if record.command.is_empty() {
        print_record_text(&record, "prepared");
    } else {
        print_record_text(&record, "started");
    }
    Ok(())
}

fn run_list(args: TaskListArgs) -> AppResult<()> {
    let paths = resolve_paths(args.cwd.as_deref())?;
    let records = read_records(&paths)?;
    if args.json {
        let items = records
            .iter()
            .map(|record| record_summary_json(record))
            .collect::<Vec<_>>();
        println!("{}", json_value_to_string(&JsonValue::Array(items)));
        return Ok(());
    }

    if records.is_empty() {
        println!(
            "no background tasks recorded for {}",
            paths.repo_root.display()
        );
        return Ok(());
    }

    println!("ID\tSTATUS\tBRANCH\tWORKTREE\tTASK");
    for record in records {
        println!(
            "{}\t{}\t{}\t{}\t{}",
            record.id,
            effective_status(&record),
            record.branch,
            record.worktree.display(),
            one_line(&record.task, 80)
        );
    }
    Ok(())
}

fn run_show(args: TaskShowArgs) -> AppResult<()> {
    let paths = resolve_paths(args.cwd.as_deref())?;
    let record = read_record(&paths, &args.id)?;
    let status = effective_status(&record);
    let git_status = git_output_optional(&record.worktree, ["status", "--short"]);
    let git_diff_stat = git_output_optional(&record.worktree, ["diff", "--stat"]);
    let stdout_tail = tail_file(&record.stdout_log, args.tail).unwrap_or_default();
    let stderr_tail = tail_file(&record.stderr_log, args.tail).unwrap_or_default();

    if args.json {
        let mut object = record_json(&record);
        object.insert(
            "effective_status".to_string(),
            JsonValue::String(status.to_string()),
        );
        object.insert(
            "git_status".to_string(),
            optional_json_string(git_status.as_deref()),
        );
        object.insert(
            "git_diff_stat".to_string(),
            optional_json_string(git_diff_stat.as_deref()),
        );
        object.insert("stdout_tail".to_string(), JsonValue::String(stdout_tail));
        object.insert("stderr_tail".to_string(), JsonValue::String(stderr_tail));
        println!("{}", json_value_to_string(&JsonValue::Object(object)));
        return Ok(());
    }

    println!("id: {}", record.id);
    println!("status: {status}");
    println!("task: {}", record.task);
    println!("repo: {}", record.repo_root.display());
    println!("worktree: {}", record.worktree.display());
    println!("branch: {}", record.branch);
    println!("base: {}", record.base_ref);
    if let Some(pid) = record.pid {
        println!("pid: {pid}");
    }
    println!("stdout: {}", record.stdout_log.display());
    println!("stderr: {}", record.stderr_log.display());
    if let Some(text) = git_status.filter(|text| !text.trim().is_empty()) {
        println!("\ngit status:\n{text}");
    }
    if let Some(text) = git_diff_stat.filter(|text| !text.trim().is_empty()) {
        println!("\ngit diff --stat:\n{text}");
    }
    if !stdout_tail.trim().is_empty() {
        println!("\nstdout tail:\n{stdout_tail}");
    }
    if !stderr_tail.trim().is_empty() {
        println!("\nstderr tail:\n{stderr_tail}");
    }
    Ok(())
}

fn run_stop(args: TaskStopArgs) -> AppResult<()> {
    let paths = resolve_paths(args.cwd.as_deref())?;
    let mut record = read_record(&paths, &args.id)?;
    let was_alive = record.pid.is_some_and(process_alive);
    if let Some(pid) = record.pid.filter(|_| was_alive) {
        terminate_process(pid)?;
    }
    record.status = "stopped".to_string();
    record.updated_at = unix_timestamp();
    write_record(&paths, &record)?;

    if args.json {
        let mut object = record_json(&record);
        object.insert("was_alive".to_string(), JsonValue::Bool(was_alive));
        println!("{}", json_value_to_string(&JsonValue::Object(object)));
    } else if was_alive {
        println!(
            "stopped task {} (pid {})",
            record.id,
            record.pid.unwrap_or(0)
        );
    } else {
        println!(
            "marked task {} stopped; no live process was found",
            record.id
        );
    }
    Ok(())
}

pub(crate) fn start_task_json(args: TaskStartArgs) -> AppResult<String> {
    let record = start_task(args)?;
    Ok(json_value_to_string(&JsonValue::Object(record_json(
        &record,
    ))))
}

fn run_diff(args: TaskDiffArgs) -> AppResult<()> {
    let paths = resolve_paths(args.cwd.as_deref())?;
    let record = read_record(&paths, &args.id)?;
    ensure_managed_worktree(&paths, &record)?;
    let patch = task_patch(&record)?;
    let stat = task_diff_stat(&record)?;
    let untracked = list_untracked_files(&record.worktree)?;

    if args.json {
        let mut object = BTreeMap::new();
        object.insert("id".to_string(), JsonValue::String(record.id));
        object.insert("patch".to_string(), JsonValue::String(patch));
        object.insert("stat".to_string(), JsonValue::String(stat));
        object.insert("untracked".to_string(), json_string_array_value(&untracked));
        println!("{}", json_value_to_string(&JsonValue::Object(object)));
        return Ok(());
    }

    if args.stat {
        if stat.trim().is_empty() && untracked.is_empty() {
            println!("task {} has no tracked or untracked diff", record.id);
        } else {
            if !stat.trim().is_empty() {
                println!("{stat}");
            }
            print_untracked(&untracked);
        }
        return Ok(());
    }

    if patch.trim().is_empty() && untracked.is_empty() {
        println!("task {} has no tracked or untracked diff", record.id);
    } else {
        if !patch.trim().is_empty() {
            println!("{patch}");
        }
        print_untracked(&untracked);
    }
    Ok(())
}

fn run_merge(args: TaskMergeArgs) -> AppResult<()> {
    let paths = resolve_paths(args.cwd.as_deref())?;
    let mut record = read_record(&paths, &args.id)?;
    let result = merge_task(&paths, &mut record, args.check, args.allow_dirty)?;
    if !args.check {
        write_record(&paths, &record)?;
    }

    if args.json {
        println!(
            "{}",
            json_value_to_string(&JsonValue::Object(merge_result_json(
                &record, &result, args.check
            )))
        );
    } else if args.check {
        println!(
            "task {} merge check passed: {} patch bytes, {} untracked files",
            record.id, result.patch_bytes, result.untracked_files
        );
    } else {
        println!(
            "merged task {} into {}: {} patch bytes, {} untracked files",
            record.id,
            paths.repo_root.display(),
            result.patch_bytes,
            result.untracked_files
        );
    }
    Ok(())
}

fn run_reject(args: TaskRejectArgs) -> AppResult<()> {
    let paths = resolve_paths(args.cwd.as_deref())?;
    let mut record = read_record(&paths, &args.id)?;
    let removed_worktree = reject_task(&paths, &mut record, args.keep_worktree)?;
    write_record(&paths, &record)?;

    if args.json {
        let mut object = record_json(&record);
        object.insert(
            "removed_worktree".to_string(),
            JsonValue::Bool(removed_worktree),
        );
        println!("{}", json_value_to_string(&JsonValue::Object(object)));
    } else if removed_worktree {
        println!("rejected task {} and removed its worktree", record.id);
    } else {
        println!("rejected task {} and kept its worktree", record.id);
    }
    Ok(())
}

fn start_task(args: TaskStartArgs) -> AppResult<TaskRecord> {
    let paths = resolve_paths(args.cwd.as_deref())?;
    fs::create_dir_all(&paths.records_dir)?;
    fs::create_dir_all(&paths.logs_dir)?;
    fs::create_dir_all(&paths.worktrees_dir)?;

    let id = args.id.clone().unwrap_or_else(generate_task_id);
    validate_task_id(&id)?;
    let branch = args
        .branch
        .clone()
        .unwrap_or_else(|| format!("deepseek-task/{id}"));
    let base_ref = args.base.clone().unwrap_or_else(|| "HEAD".to_string());
    let record_path = record_path(&paths, &id)?;
    if record_path.exists() {
        return Err(app_error(format!("task id already exists: {id}")));
    }

    let worktree = paths.worktrees_dir.join(&id);
    if worktree.exists() {
        return Err(app_error(format!(
            "task worktree already exists: {}",
            worktree.display()
        )));
    }
    let stdout_log = paths.logs_dir.join(format!("{id}.stdout.log"));
    let stderr_log = paths.logs_dir.join(format!("{id}.stderr.log"));

    run_git_checked(
        &paths.repo_root,
        [
            OsStr::new("worktree"),
            OsStr::new("add"),
            OsStr::new("-b"),
            OsStr::new(&branch),
            worktree.as_os_str(),
            OsStr::new(&base_ref),
        ],
    )?;

    let mut command = Vec::new();
    let pid = if args.no_run {
        None
    } else {
        let exe = std::env::current_exe()?;
        let stdout = File::create(&stdout_log)?;
        let stderr = File::create(&stderr_log)?;
        let mut child = Command::new(&exe);
        child.arg("exec").arg("--json");
        if let Some(skill) = &args.skill {
            child.arg("--skill").arg(skill);
        }
        if let Some(budget) = args.budget {
            child.arg("--budget").arg(budget.to_string());
        }
        child.arg("--").arg(&args.task);
        child
            .current_dir(&worktree)
            .stdin(Stdio::null())
            .stdout(Stdio::from(stdout))
            .stderr(Stdio::from(stderr));
        #[cfg(unix)]
        {
            child.process_group(0);
        }
        command = command_to_vec(&exe, &args);
        Some(child.spawn()?.id())
    };

    if args.no_run {
        File::create(&stdout_log)?;
        File::create(&stderr_log)?;
    }

    let now = unix_timestamp();
    let record = TaskRecord {
        id,
        task: args.task,
        status: if args.no_run { "pending" } else { "running" }.to_string(),
        repo_root: paths.repo_root.clone(),
        worktree,
        branch,
        base_ref,
        pid,
        exit_code: None,
        stdout_log,
        stderr_log,
        created_at: now,
        updated_at: now,
        skill: args.skill,
        budget: args.budget,
        command,
    };
    write_record(&paths, &record)?;
    Ok(record)
}

fn merge_task(
    paths: &TaskPaths,
    record: &mut TaskRecord,
    check: bool,
    allow_dirty: bool,
) -> AppResult<MergeResult> {
    ensure_managed_worktree(paths, record)?;
    if effective_status(record) == "running" {
        return Err(app_error(format!(
            "task {} is still running; stop it or wait before merging",
            record.id
        )));
    }
    if !allow_dirty && repo_is_dirty(&paths.repo_root)? {
        return Err(app_error(
            "refusing to merge task into a dirty worktree; commit/stash changes or pass --allow-dirty",
        ));
    }

    let patch = task_patch(record)?;
    let untracked = list_untracked_files(&record.worktree)?;
    if !patch.trim().is_empty() {
        git_apply_stdin(&paths.repo_root, &patch, true)?;
    }
    check_untracked_targets(paths, record, &untracked)?;

    if !check {
        if !patch.trim().is_empty() {
            git_apply_stdin(&paths.repo_root, &patch, false)?;
        }
        copy_untracked_files(paths, record, &untracked)?;
        record.status = "merged".to_string();
        record.updated_at = unix_timestamp();
    }

    Ok(MergeResult {
        patch_bytes: patch.len(),
        untracked_files: untracked.len(),
    })
}

fn reject_task(paths: &TaskPaths, record: &mut TaskRecord, keep_worktree: bool) -> AppResult<bool> {
    ensure_managed_record(paths, record)?;
    let mut removed_worktree = false;
    if !keep_worktree && record.worktree.exists() {
        ensure_managed_worktree(paths, record)?;
        run_git_checked(
            &paths.repo_root,
            [
                OsStr::new("worktree"),
                OsStr::new("remove"),
                OsStr::new("--force"),
                record.worktree.as_os_str(),
            ],
        )?;
        removed_worktree = true;
    }
    record.status = "rejected".to_string();
    record.updated_at = unix_timestamp();
    Ok(removed_worktree)
}

fn run_fixture_smoke(args: TaskFixtureSmokeArgs) -> AppResult<()> {
    let smoke_root = std::env::temp_dir().join(format!("deepseek-task-smoke-{}", unique_suffix()));
    fs::create_dir_all(&smoke_root)?;
    let repo = smoke_root.join("repo");
    fs::create_dir_all(&repo)?;
    run_git_checked(&repo, ["init"])?;
    run_git_checked(&repo, ["config", "user.email", "deepseek@example.invalid"])?;
    run_git_checked(&repo, ["config", "user.name", "DeepSeek Task Smoke"])?;
    fs::write(repo.join("README.md"), "task smoke\n")?;
    fs::write(repo.join(".gitignore"), ".dscode/\n")?;
    run_git_checked(&repo, ["add", "README.md", ".gitignore"])?;
    run_git_checked(&repo, ["commit", "-m", "init"])?;

    let record = start_task(TaskStartArgs {
        task: "prepare an isolated worktree".to_string(),
        cwd: Some(path_string(&repo)),
        id: Some("smoke-task".to_string()),
        no_run: true,
        json: true,
        ..TaskStartArgs::default()
    })?;
    let records = read_records(&resolve_paths(Some(&path_string(&repo)))?)?;
    let worktree_created = record.worktree.is_dir();
    let show_ok = worktree_created
        && records.iter().any(|item| item.id == record.id)
        && effective_status(&record) == "pending";
    fs::write(record.worktree.join("README.md"), "task smoke\nmerged\n")?;
    fs::write(record.worktree.join("notes.txt"), "new note\n")?;
    let paths = resolve_paths(Some(&path_string(&repo)))?;
    let mut merge_record = read_record(&paths, "smoke-task")?;
    let merge_check = merge_task(&paths, &mut merge_record, true, false)?;
    let merge_check_ok = merge_check.patch_bytes > 0 && merge_check.untracked_files == 1;
    let merge_apply = merge_task(&paths, &mut merge_record, false, false)?;
    write_record(&paths, &merge_record)?;
    let merge_apply_ok = merge_apply.patch_bytes > 0
        && merge_apply.untracked_files == 1
        && fs::read_to_string(repo.join("README.md"))?.contains("merged")
        && repo.join("notes.txt").is_file()
        && merge_record.status == "merged";

    let reject_record = start_task(TaskStartArgs {
        task: "prepare a rejected worktree".to_string(),
        cwd: Some(path_string(&repo)),
        id: Some("reject-task".to_string()),
        no_run: true,
        json: true,
        ..TaskStartArgs::default()
    })?;
    let mut reject_loaded = read_record(&paths, &reject_record.id)?;
    let reject_removed = reject_task(&paths, &mut reject_loaded, false)?;
    write_record(&paths, &reject_loaded)?;
    let reject_ok =
        reject_removed && !reject_record.worktree.exists() && reject_loaded.status == "rejected";

    let cleanup_ok = if args.keep_workdir {
        true
    } else {
        fs::remove_dir_all(&smoke_root).is_ok()
    };

    let ok = show_ok && merge_check_ok && merge_apply_ok && reject_ok && cleanup_ok;
    let mut object = BTreeMap::new();
    object.insert(
        "schema".to_string(),
        JsonValue::String(SMOKE_SCHEMA.to_string()),
    );
    object.insert("ok".to_string(), JsonValue::Bool(ok));
    object.insert(
        "worktree_created".to_string(),
        JsonValue::Bool(worktree_created),
    );
    object.insert("record_listed".to_string(), JsonValue::Bool(show_ok));
    object.insert(
        "merge_check_ok".to_string(),
        JsonValue::Bool(merge_check_ok),
    );
    object.insert(
        "merge_apply_ok".to_string(),
        JsonValue::Bool(merge_apply_ok),
    );
    object.insert("reject_ok".to_string(), JsonValue::Bool(reject_ok));
    object.insert("cleanup_ok".to_string(), JsonValue::Bool(cleanup_ok));
    object.insert("task_id".to_string(), JsonValue::String(record.id));
    object.insert(
        "workdir".to_string(),
        JsonValue::String(path_string(&smoke_root)),
    );

    if args.json {
        println!("{}", json_value_to_string(&JsonValue::Object(object)));
    } else if ok {
        println!(
            "task fixture smoke passed: worktree {}",
            record.worktree.display()
        );
    } else {
        println!("{}", json_value_to_string(&JsonValue::Object(object)));
    }

    if ok {
        Ok(())
    } else {
        Err(app_error("task fixture smoke failed"))
    }
}

fn resolve_paths(cwd: Option<&str>) -> AppResult<TaskPaths> {
    let cwd = match cwd {
        Some(path) => PathBuf::from(path),
        None => std::env::current_dir()?,
    };
    let repo_root = PathBuf::from(run_git_checked(&cwd, ["rev-parse", "--show-toplevel"])?.trim());
    let storage = repo_root.join(".dscode").join("task-runner");
    Ok(TaskPaths {
        repo_root,
        records_dir: storage.join("records"),
        logs_dir: storage.join("logs"),
        worktrees_dir: storage.join("worktrees"),
    })
}

fn ensure_managed_record(paths: &TaskPaths, record: &TaskRecord) -> AppResult<()> {
    if record.repo_root != paths.repo_root {
        return Err(app_error(format!(
            "task {} belongs to {}, not {}",
            record.id,
            record.repo_root.display(),
            paths.repo_root.display()
        )));
    }
    if !record.worktree.starts_with(&paths.worktrees_dir) {
        return Err(app_error(format!(
            "task {} worktree is outside managed task directory: {}",
            record.id,
            record.worktree.display()
        )));
    }
    Ok(())
}

fn ensure_managed_worktree(paths: &TaskPaths, record: &TaskRecord) -> AppResult<()> {
    ensure_managed_record(paths, record)?;
    if !record.worktree.is_dir() {
        return Err(app_error(format!(
            "task {} worktree is missing: {}",
            record.id,
            record.worktree.display()
        )));
    }
    Ok(())
}

fn repo_is_dirty(repo_root: &Path) -> AppResult<bool> {
    Ok(!run_git_checked(repo_root, ["status", "--porcelain"])?
        .trim()
        .is_empty())
}

fn task_patch(record: &TaskRecord) -> AppResult<String> {
    git_text_output(&record.worktree, ["diff", "--binary", "HEAD"])
}

fn task_diff_stat(record: &TaskRecord) -> AppResult<String> {
    git_text_output(&record.worktree, ["diff", "--stat", "HEAD"])
}

fn list_untracked_files(worktree: &Path) -> AppResult<Vec<String>> {
    let bytes = run_git_bytes_checked(
        worktree,
        ["ls-files", "--others", "--exclude-standard", "-z"],
    )?;
    let mut paths = Vec::new();
    for raw in bytes.split(|byte| *byte == 0) {
        if raw.is_empty() {
            continue;
        }
        let path = String::from_utf8(raw.to_vec())
            .map_err(|_| app_error("git returned a non-utf8 untracked path"))?;
        validate_relative_git_path(&path)?;
        paths.push(path);
    }
    paths.sort();
    Ok(paths)
}

fn validate_relative_git_path(path: &str) -> AppResult<()> {
    let candidate = Path::new(path);
    if candidate.is_absolute()
        || candidate
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return Err(app_error(format!(
            "unsafe git path in task worktree: {path}"
        )));
    }
    Ok(())
}

fn check_untracked_targets(
    paths: &TaskPaths,
    record: &TaskRecord,
    untracked: &[String],
) -> AppResult<()> {
    for path in untracked {
        let source = record.worktree.join(path);
        let target = paths.repo_root.join(path);
        let metadata = fs::symlink_metadata(&source)?;
        if !metadata.file_type().is_file() {
            return Err(app_error(format!(
                "untracked task path is not a regular file: {path}"
            )));
        }
        if target.exists() {
            return Err(app_error(format!(
                "refusing to overwrite existing untracked merge target: {path}"
            )));
        }
    }
    Ok(())
}

fn copy_untracked_files(
    paths: &TaskPaths,
    record: &TaskRecord,
    untracked: &[String],
) -> AppResult<()> {
    for path in untracked {
        let source = record.worktree.join(path);
        let target = paths.repo_root.join(path);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::copy(source, target)?;
    }
    Ok(())
}

fn read_records(paths: &TaskPaths) -> AppResult<Vec<TaskRecord>> {
    if !paths.records_dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut records = Vec::new();
    for entry in fs::read_dir(&paths.records_dir)? {
        let entry = entry?;
        if entry.path().extension().and_then(|value| value.to_str()) != Some("json") {
            continue;
        }
        let content = fs::read_to_string(entry.path())?;
        records.push(record_from_json(&content)?);
    }
    records.sort_by(|left, right| right.created_at.cmp(&left.created_at));
    Ok(records)
}

fn read_record(paths: &TaskPaths, id: &str) -> AppResult<TaskRecord> {
    let path = record_path(paths, id)?;
    let content =
        fs::read_to_string(&path).map_err(|_| app_error(format!("task record not found: {id}")))?;
    record_from_json(&content)
}

fn write_record(paths: &TaskPaths, record: &TaskRecord) -> AppResult<()> {
    fs::create_dir_all(&paths.records_dir)?;
    let path = record_path(paths, &record.id)?;
    fs::write(
        path,
        format!(
            "{}\n",
            json_value_to_string(&JsonValue::Object(record_json(record)))
        ),
    )?;
    Ok(())
}

fn record_path(paths: &TaskPaths, id: &str) -> AppResult<PathBuf> {
    validate_task_id(id)?;
    Ok(paths.records_dir.join(format!("{id}.json")))
}

fn record_json(record: &TaskRecord) -> BTreeMap<String, JsonValue> {
    let mut object = BTreeMap::new();
    object.insert(
        "schema".to_string(),
        JsonValue::String(TASK_SCHEMA.to_string()),
    );
    object.insert("id".to_string(), JsonValue::String(record.id.clone()));
    object.insert("task".to_string(), JsonValue::String(record.task.clone()));
    object.insert(
        "status".to_string(),
        JsonValue::String(record.status.clone()),
    );
    object.insert(
        "repo_root".to_string(),
        JsonValue::String(path_string(&record.repo_root)),
    );
    object.insert(
        "worktree".to_string(),
        JsonValue::String(path_string(&record.worktree)),
    );
    object.insert(
        "branch".to_string(),
        JsonValue::String(record.branch.clone()),
    );
    object.insert(
        "base_ref".to_string(),
        JsonValue::String(record.base_ref.clone()),
    );
    object.insert(
        "pid".to_string(),
        record
            .pid
            .map(|pid| JsonValue::Number(pid.to_string()))
            .unwrap_or(JsonValue::Null),
    );
    object.insert(
        "exit_code".to_string(),
        record
            .exit_code
            .map(|code| JsonValue::Number(code.to_string()))
            .unwrap_or(JsonValue::Null),
    );
    object.insert(
        "stdout_log".to_string(),
        JsonValue::String(path_string(&record.stdout_log)),
    );
    object.insert(
        "stderr_log".to_string(),
        JsonValue::String(path_string(&record.stderr_log)),
    );
    object.insert(
        "created_at".to_string(),
        JsonValue::Number(record.created_at.to_string()),
    );
    object.insert(
        "updated_at".to_string(),
        JsonValue::Number(record.updated_at.to_string()),
    );
    object.insert(
        "skill".to_string(),
        record
            .skill
            .as_ref()
            .map(|value| JsonValue::String(value.clone()))
            .unwrap_or(JsonValue::Null),
    );
    object.insert(
        "budget".to_string(),
        record
            .budget
            .map(|value| JsonValue::Number(value.to_string()))
            .unwrap_or(JsonValue::Null),
    );
    object.insert(
        "command".to_string(),
        JsonValue::Array(
            record
                .command
                .iter()
                .map(|value| JsonValue::String(value.clone()))
                .collect(),
        ),
    );
    object
}

fn record_summary_json(record: &TaskRecord) -> JsonValue {
    let mut object = BTreeMap::new();
    object.insert("id".to_string(), JsonValue::String(record.id.clone()));
    object.insert(
        "status".to_string(),
        JsonValue::String(effective_status(record).to_string()),
    );
    object.insert("task".to_string(), JsonValue::String(record.task.clone()));
    object.insert(
        "branch".to_string(),
        JsonValue::String(record.branch.clone()),
    );
    object.insert(
        "worktree".to_string(),
        JsonValue::String(path_string(&record.worktree)),
    );
    object.insert(
        "created_at".to_string(),
        JsonValue::Number(record.created_at.to_string()),
    );
    JsonValue::Object(object)
}

fn record_from_json(content: &str) -> AppResult<TaskRecord> {
    let root = parse_root_object(content)?;
    if json_string(&root, "schema")? != TASK_SCHEMA {
        return Err(app_error("unsupported task record schema"));
    }
    Ok(TaskRecord {
        id: json_string(&root, "id")?,
        task: json_string(&root, "task")?,
        status: json_string(&root, "status")?,
        repo_root: PathBuf::from(json_string(&root, "repo_root")?),
        worktree: PathBuf::from(json_string(&root, "worktree")?),
        branch: json_string(&root, "branch")?,
        base_ref: json_string(&root, "base_ref")?,
        pid: json_u32_opt(&root, "pid")?,
        exit_code: json_i32_opt(&root, "exit_code")?,
        stdout_log: PathBuf::from(json_string(&root, "stdout_log")?),
        stderr_log: PathBuf::from(json_string(&root, "stderr_log")?),
        created_at: json_u64(&root, "created_at")?,
        updated_at: json_u64(&root, "updated_at")?,
        skill: json_string_opt(&root, "skill")?,
        budget: json_usize_opt(&root, "budget")?,
        command: json_string_array(&root, "command")?,
    })
}

fn json_string(root: &BTreeMap<String, JsonValue>, key: &str) -> AppResult<String> {
    match root.get(key) {
        Some(JsonValue::String(value)) => Ok(value.clone()),
        _ => Err(app_error(format!(
            "task record field `{key}` must be a string"
        ))),
    }
}

fn json_string_opt(root: &BTreeMap<String, JsonValue>, key: &str) -> AppResult<Option<String>> {
    match root.get(key) {
        Some(JsonValue::String(value)) => Ok(Some(value.clone())),
        Some(JsonValue::Null) | None => Ok(None),
        _ => Err(app_error(format!(
            "task record field `{key}` must be a string or null"
        ))),
    }
}

fn json_u64(root: &BTreeMap<String, JsonValue>, key: &str) -> AppResult<u64> {
    match root.get(key) {
        Some(JsonValue::Number(value)) => value
            .parse::<u64>()
            .map_err(|_| app_error(format!("task record field `{key}` must be a number"))),
        _ => Err(app_error(format!(
            "task record field `{key}` must be a number"
        ))),
    }
}

fn json_u32_opt(root: &BTreeMap<String, JsonValue>, key: &str) -> AppResult<Option<u32>> {
    json_number_opt(root, key, |value| value.parse::<u32>().ok())
}

fn json_i32_opt(root: &BTreeMap<String, JsonValue>, key: &str) -> AppResult<Option<i32>> {
    json_number_opt(root, key, |value| value.parse::<i32>().ok())
}

fn json_usize_opt(root: &BTreeMap<String, JsonValue>, key: &str) -> AppResult<Option<usize>> {
    json_number_opt(root, key, |value| value.parse::<usize>().ok())
}

fn json_number_opt<T>(
    root: &BTreeMap<String, JsonValue>,
    key: &str,
    parse: impl FnOnce(&str) -> Option<T>,
) -> AppResult<Option<T>> {
    match root.get(key) {
        Some(JsonValue::Number(value)) => parse(value)
            .map(Some)
            .ok_or_else(|| app_error(format!("task record field `{key}` must be a number"))),
        Some(JsonValue::Null) | None => Ok(None),
        _ => Err(app_error(format!(
            "task record field `{key}` must be a number or null"
        ))),
    }
}

fn json_string_array(root: &BTreeMap<String, JsonValue>, key: &str) -> AppResult<Vec<String>> {
    match root.get(key) {
        Some(JsonValue::Array(items)) => items
            .iter()
            .map(|item| match item {
                JsonValue::String(value) => Ok(value.clone()),
                _ => Err(app_error(format!(
                    "task record field `{key}` must contain only strings"
                ))),
            })
            .collect(),
        Some(JsonValue::Null) | None => Ok(Vec::new()),
        _ => Err(app_error(format!(
            "task record field `{key}` must be an array"
        ))),
    }
}

fn run_git_checked<I, S>(cwd: &Path, args: I) -> AppResult<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let args = args.into_iter().collect::<Vec<_>>();
    let output = Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args(&args)
        .output()?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(app_error(format!(
            "git command failed in {}: {}",
            cwd.display(),
            stderr.trim()
        )));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn run_git_bytes_checked<I, S>(cwd: &Path, args: I) -> AppResult<Vec<u8>>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let args = args.into_iter().collect::<Vec<_>>();
    let output = Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args(&args)
        .output()?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(app_error(format!(
            "git command failed in {}: {}",
            cwd.display(),
            stderr.trim()
        )));
    }
    Ok(output.stdout)
}

fn git_apply_stdin(cwd: &Path, patch: &str, check: bool) -> AppResult<()> {
    let mut command = Command::new("git");
    command.arg("-C").arg(cwd).arg("apply");
    if check {
        command.arg("--check");
    }
    command.arg("-");
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    {
        let Some(mut stdin) = child.stdin.take() else {
            return Err(app_error("failed to open git apply stdin"));
        };
        stdin.write_all(patch.as_bytes())?;
    }
    let output = child.wait_with_output()?;
    if output.status.success() {
        Ok(())
    } else {
        Err(app_error(format!(
            "git apply failed in {}: {}",
            cwd.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        )))
    }
}

fn git_output_optional<I, S>(cwd: &Path, args: I) -> Option<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    run_git_checked(cwd, args).ok()
}

fn git_text_output<I, S>(cwd: &Path, args: I) -> AppResult<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let bytes = run_git_bytes_checked(cwd, args)?;
    String::from_utf8(bytes).map_err(|_| app_error("git returned non-utf8 text output"))
}

fn command_to_vec(exe: &Path, args: &TaskStartArgs) -> Vec<String> {
    let mut command = vec![path_string(exe), "exec".to_string(), "--json".to_string()];
    if let Some(skill) = &args.skill {
        command.push("--skill".to_string());
        command.push(skill.clone());
    }
    if let Some(budget) = args.budget {
        command.push("--budget".to_string());
        command.push(budget.to_string());
    }
    command.push("--".to_string());
    command.push(args.task.clone());
    command
}

fn effective_status(record: &TaskRecord) -> &str {
    if record.status == "running" {
        if let Some(pid) = record.pid {
            if process_alive(pid) {
                return "running";
            }
        }
        return "exited";
    }
    &record.status
}

#[cfg(unix)]
fn process_alive(pid: u32) -> bool {
    Command::new("kill")
        .arg("-0")
        .arg(pid.to_string())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

#[cfg(windows)]
fn process_alive(pid: u32) -> bool {
    let filter = format!("PID eq {pid}");
    let output = Command::new("tasklist")
        .arg("/FI")
        .arg(filter)
        .arg("/NH")
        .output();
    output
        .map(|output| {
            output.status.success()
                && String::from_utf8_lossy(&output.stdout)
                    .to_ascii_lowercase()
                    .contains(&pid.to_string())
        })
        .unwrap_or(false)
}

#[cfg(not(any(unix, windows)))]
fn process_alive(_pid: u32) -> bool {
    false
}

#[cfg(unix)]
fn terminate_process(pid: u32) -> AppResult<()> {
    let status = Command::new("kill")
        .arg("-TERM")
        .arg(format!("-{pid}"))
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(app_error(format!("failed to terminate pid {pid}")))
    }
}

#[cfg(windows)]
fn terminate_process(pid: u32) -> AppResult<()> {
    let status = Command::new("taskkill")
        .arg("/PID")
        .arg(pid.to_string())
        .arg("/T")
        .arg("/F")
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(app_error(format!("failed to terminate pid {pid}")))
    }
}

#[cfg(not(any(unix, windows)))]
fn terminate_process(pid: u32) -> AppResult<()> {
    Err(app_error(format!(
        "stopping background task pid {pid} is unsupported on this platform"
    )))
}

fn tail_file(path: &Path, limit: usize) -> AppResult<String> {
    let content = fs::read_to_string(path)?;
    let lines = content.lines().collect::<Vec<_>>();
    let start = lines.len().saturating_sub(limit);
    Ok(lines[start..].join("\n"))
}

fn print_record_text(record: &TaskRecord, verb: &str) {
    println!("{verb} task {}", record.id);
    println!("status: {}", record.status);
    println!("worktree: {}", record.worktree.display());
    println!("branch: {}", record.branch);
    if let Some(pid) = record.pid {
        println!("pid: {pid}");
    }
    println!("stdout: {}", record.stdout_log.display());
    println!("stderr: {}", record.stderr_log.display());
}

fn optional_json_string(value: Option<&str>) -> JsonValue {
    value
        .map(|value| JsonValue::String(value.to_string()))
        .unwrap_or(JsonValue::Null)
}

fn json_string_array_value(values: &[String]) -> JsonValue {
    JsonValue::Array(
        values
            .iter()
            .map(|value| JsonValue::String(value.clone()))
            .collect(),
    )
}

fn merge_result_json(
    record: &TaskRecord,
    result: &MergeResult,
    checked_only: bool,
) -> BTreeMap<String, JsonValue> {
    let mut object = BTreeMap::new();
    object.insert("id".to_string(), JsonValue::String(record.id.clone()));
    object.insert(
        "status".to_string(),
        JsonValue::String(record.status.clone()),
    );
    object.insert("checked_only".to_string(), JsonValue::Bool(checked_only));
    object.insert(
        "patch_bytes".to_string(),
        JsonValue::Number(result.patch_bytes.to_string()),
    );
    object.insert(
        "untracked_files".to_string(),
        JsonValue::Number(result.untracked_files.to_string()),
    );
    object
}

fn print_untracked(paths: &[String]) {
    if paths.is_empty() {
        return;
    }
    println!("untracked files:");
    for path in paths {
        println!("  {path}");
    }
}

fn validate_task_id(id: &str) -> AppResult<()> {
    if id.is_empty() || id == "." || id == ".." || id.len() > 64 {
        return Err(app_error(
            "task id must be 1-64 safe characters, not `.` or `..`",
        ));
    }
    if !id
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        return Err(app_error(
            "task id may only contain ASCII letters, numbers, dash, underscore, or dot",
        ));
    }
    Ok(())
}

fn generate_task_id() -> String {
    format!("task-{}-{}", unix_timestamp(), std::process::id())
}

fn unix_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

fn unique_suffix() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    format!("{}-{nanos}", std::process::id())
}

fn path_string(path: &Path) -> String {
    path.to_string_lossy().to_string()
}

fn one_line(value: &str, max_chars: usize) -> String {
    let mut output = value.replace(['\n', '\r', '\t'], " ");
    if output.chars().count() > max_chars {
        output = output.chars().take(max_chars.saturating_sub(3)).collect();
        output.push_str("...");
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_record_round_trips_json() {
        let record = TaskRecord {
            id: "task-1".to_string(),
            task: "fix tests".to_string(),
            status: "pending".to_string(),
            repo_root: PathBuf::from("/tmp/repo"),
            worktree: PathBuf::from("/tmp/repo/.dscode/task-runner/worktrees/task-1"),
            branch: "deepseek-task/task-1".to_string(),
            base_ref: "HEAD".to_string(),
            pid: None,
            exit_code: None,
            stdout_log: PathBuf::from("/tmp/stdout.log"),
            stderr_log: PathBuf::from("/tmp/stderr.log"),
            created_at: 1,
            updated_at: 2,
            skill: Some("review".to_string()),
            budget: Some(10),
            command: vec!["deepseek".to_string(), "exec".to_string()],
        };
        let encoded = json_value_to_string(&JsonValue::Object(record_json(&record)));
        let decoded = record_from_json(&encoded).unwrap();
        assert_eq!(decoded, record);
    }

    #[test]
    fn validate_task_id_rejects_path_escape() {
        assert!(validate_task_id("good-id_1.2").is_ok());
        assert!(validate_task_id("../bad").is_err());
        assert!(validate_task_id("").is_err());
    }
}
