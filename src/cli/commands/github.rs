use crate::cli::app::{
    GithubAction, GithubActionArgs, GithubActionMode, GithubFixtureSmokeArgs,
    GithubFixtureSmokeMode, GithubPrHeadArgs, PrAction, TaskStartArgs,
};
use crate::error::{app_error, AppResult};
use crate::util::json::{
    json_as_object, json_as_string, json_as_u64, json_escape, parse_root_object, JsonValue,
};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, PartialEq, Eq)]
struct GithubActionTarget {
    event_name: String,
    repo: String,
    number: u64,
    mode: GithubActionMode,
    trigger_matched: bool,
    reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct GithubPrHeadTarget {
    reference: String,
    head_owner: String,
    head_ref: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct GhPrViewSelector {
    reference_arg: String,
    repo: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct GithubFixtureSmokeReport {
    workdir: PathBuf,
    cleanup: bool,
    review: Option<GithubFixtureReviewSmoke>,
    write: Option<GithubFixtureWriteSmoke>,
    background_task: Option<GithubFixtureBackgroundTaskSmoke>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct GithubFixtureReviewSmoke {
    reference: String,
    mode: GithubActionMode,
    trigger_matched: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct GithubFixtureWriteSmoke {
    reference: String,
    mode: GithubActionMode,
    head_ref: String,
    fork_guard_verified: bool,
    pushed_head: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct GithubFixtureBackgroundTaskSmoke {
    reference: String,
    mode: GithubActionMode,
    task_id: String,
    task_record_created: bool,
    worktree_created: bool,
}

pub fn run(action: GithubAction) -> AppResult<()> {
    match action {
        GithubAction::Action(args) => run_action(args),
        GithubAction::PrHead(args) => run_pr_head(args),
        GithubAction::FixtureSmoke(args) => run_fixture_smoke(args),
    }
}

fn run_action(args: GithubActionArgs) -> AppResult<()> {
    if args.github_output && !args.dry_run {
        return Err(app_error("--github-output requires --dry-run"));
    }
    let event_path = args
        .event_path
        .or_else(|| std::env::var("GITHUB_EVENT_PATH").ok())
        .ok_or_else(|| app_error("github action requires --event <path> or GITHUB_EVENT_PATH"))?;
    let event_name = args
        .event_name
        .or_else(|| std::env::var("GITHUB_EVENT_NAME").ok())
        .ok_or_else(|| {
            app_error("github action requires --event-name <name> or GITHUB_EVENT_NAME")
        })?;
    let body = std::fs::read_to_string(&event_path)?;
    let target = github_action_target_from_event(
        &event_name,
        &body,
        args.mode,
        &args.trigger,
        args.allow_untriggered,
    )?;
    validate_required_modes(&target, &args.require_modes)?;
    let reference = format!("{}#{}", target.repo, target.number);
    if args.dry_run {
        if args.github_output {
            write_github_output(&target, args.post)?;
        }
        println!("{}", render_action_target_json(&target, args.post));
        return Ok(());
    }
    if args.background_task {
        let rendered = start_github_background_task(
            &target,
            &reference,
            args.post,
            args.job.as_deref(),
            args.commit,
            args.task_no_run,
            args.task_id,
            None,
        )?;
        println!("{rendered}");
        return Ok(());
    }
    let action = match target.mode {
        GithubActionMode::Auto => unreachable!("auto mode should be resolved before dispatch"),
        GithubActionMode::Review => PrAction::Review {
            reference,
            post: args.post,
            out: None,
        },
        GithubActionMode::Fix => PrAction::Fix {
            reference,
            job: args.job,
            benchmark_gate: false,
        },
        GithubActionMode::Patch => PrAction::Patch {
            reference,
            commit: args.commit,
            benchmark_gate: false,
        },
    };
    crate::cli::commands::pr::run(action)
}

fn run_pr_head(args: GithubPrHeadArgs) -> AppResult<()> {
    let GithubPrHeadArgs {
        reference,
        repo_owner,
        github_output,
        json_file,
    } = args;
    let body = match json_file {
        Some(path) => std::fs::read_to_string(&path)
            .map_err(|error| app_error(format!("failed to read --json-file {path}: {error}")))?,
        None => fetch_pr_head_json(&reference)?,
    };
    let target = parse_pr_head_target(&reference, &body)?;
    if let Some(repo_owner) = repo_owner.as_deref() {
        validate_pr_head_owner(&target, repo_owner)?;
    }
    if github_output {
        write_pr_head_github_output(&target)?;
    }
    println!("{}", render_pr_head_json(&target));
    Ok(())
}

fn run_fixture_smoke(args: GithubFixtureSmokeArgs) -> AppResult<()> {
    let workdir = github_fixture_temp_root()?;
    fs::create_dir_all(&workdir)?;
    let result = run_fixture_smoke_in_workdir(&workdir, args.mode);
    if result.is_ok() && !args.keep_workdir {
        fs::remove_dir_all(&workdir).map_err(|error| {
            app_error(format!(
                "GitHub fixture smoke passed, but failed to remove {}: {error}",
                workdir.display()
            ))
        })?;
    }
    let mut report = result?;
    report.cleanup = !args.keep_workdir;
    if args.json {
        println!("{}", render_fixture_smoke_json(&report));
    } else {
        print_fixture_smoke_report(&report);
    }
    Ok(())
}

fn run_fixture_smoke_in_workdir(
    workdir: &Path,
    mode: GithubFixtureSmokeMode,
) -> AppResult<GithubFixtureSmokeReport> {
    let review = if matches!(
        mode,
        GithubFixtureSmokeMode::All | GithubFixtureSmokeMode::Review
    ) {
        Some(run_fixture_review_smoke()?)
    } else {
        None
    };
    let write = if matches!(
        mode,
        GithubFixtureSmokeMode::All | GithubFixtureSmokeMode::Write
    ) {
        Some(run_fixture_write_smoke(workdir)?)
    } else {
        None
    };
    let background_task = if matches!(
        mode,
        GithubFixtureSmokeMode::All | GithubFixtureSmokeMode::Write
    ) {
        Some(run_fixture_background_task_smoke(workdir)?)
    } else {
        None
    };
    Ok(GithubFixtureSmokeReport {
        workdir: workdir.to_path_buf(),
        cleanup: false,
        review,
        write,
        background_task,
    })
}

fn run_fixture_review_smoke() -> AppResult<GithubFixtureReviewSmoke> {
    let target = github_action_target_from_event(
        "issue_comment",
        &fixture_issue_comment_event("@deepseek review this PR"),
        GithubActionMode::Auto,
        "@deepseek",
        false,
    )?;
    validate_required_modes(&target, &[GithubActionMode::Review])?;
    Ok(GithubFixtureReviewSmoke {
        reference: format!("{}#{}", target.repo, target.number),
        mode: target.mode,
        trigger_matched: target.trigger_matched,
    })
}

fn run_fixture_write_smoke(workdir: &Path) -> AppResult<GithubFixtureWriteSmoke> {
    let target = github_action_target_from_event(
        "issue_comment",
        &fixture_issue_comment_event("@deepseek fix the failing fixture"),
        GithubActionMode::Auto,
        "@deepseek",
        false,
    )?;
    validate_required_modes(&target, &[GithubActionMode::Fix, GithubActionMode::Patch])?;
    let reference = format!("{}#{}", target.repo, target.number);
    let pr_head = parse_pr_head_target(
        &reference,
        r#"{"headRepositoryOwner":{"login":"owner"},"headRefName":"feature/deepseek"}"#,
    )?;
    validate_pr_head_owner(&pr_head, "owner")?;
    let fork_guard_verified = validate_pr_head_owner(
        &GithubPrHeadTarget {
            reference: reference.clone(),
            head_owner: "fork".to_string(),
            head_ref: pr_head.head_ref.clone(),
        },
        "owner",
    )
    .is_err();
    if !fork_guard_verified {
        return Err(app_error(
            "GitHub fixture smoke expected fork-owned PR head to be rejected",
        ));
    }
    let pushed_head = run_fixture_checkout_commit_push(workdir, &pr_head.head_ref)?;
    Ok(GithubFixtureWriteSmoke {
        reference,
        mode: target.mode,
        head_ref: pr_head.head_ref,
        fork_guard_verified,
        pushed_head,
    })
}

fn run_fixture_background_task_smoke(
    workdir: &Path,
) -> AppResult<GithubFixtureBackgroundTaskSmoke> {
    let repo = workdir.join("background-task-repo");
    fs::create_dir_all(&repo)?;
    run_git(&repo, &["init"])?;
    run_git(&repo, &["config", "user.name", "deepseek-code-fixture"])?;
    run_git(
        &repo,
        &[
            "config",
            "user.email",
            "deepseek-code-fixture@example.invalid",
        ],
    )?;
    fs::write(repo.join(".gitignore"), ".dscode/\n")?;
    fs::write(
        repo.join("README.md"),
        "DeepSeekCode background task fixture\n",
    )?;
    run_git(&repo, &["add", ".gitignore", "README.md"])?;
    run_git(&repo, &["commit", "-m", "initial background task fixture"])?;

    let target = github_action_target_from_event(
        "issue_comment",
        &fixture_issue_comment_event("@deepseek fix the background task"),
        GithubActionMode::Auto,
        "@deepseek",
        false,
    )?;
    validate_required_modes(&target, &[GithubActionMode::Fix, GithubActionMode::Patch])?;
    let reference = format!("{}#{}", target.repo, target.number);
    let task_id = "github-bg-smoke".to_string();
    let rendered = start_github_background_task(
        &target,
        &reference,
        false,
        Some("fixture-ci"),
        false,
        true,
        Some(task_id.clone()),
        Some(path_string(&repo)),
    )?;
    let task_object = parse_root_object(&rendered)?;
    if string_field(&task_object, "id")? != task_id {
        return Err(app_error(
            "GitHub background task smoke returned unexpected task id",
        ));
    }
    let record_path = repo
        .join(".dscode")
        .join("task-runner")
        .join("records")
        .join(format!("{task_id}.json"));
    let worktree_path = repo
        .join(".dscode")
        .join("task-runner")
        .join("worktrees")
        .join(&task_id);
    let record_body = fs::read_to_string(&record_path).unwrap_or_default();
    let task_record_created = record_path.is_file()
        && record_body.contains(&reference)
        && record_body.contains("fixture-ci");
    let worktree_created = worktree_path.is_dir();
    if !task_record_created || !worktree_created {
        return Err(app_error(
            "GitHub background task smoke did not create the expected task record/worktree",
        ));
    }

    Ok(GithubFixtureBackgroundTaskSmoke {
        reference,
        mode: target.mode,
        task_id,
        task_record_created,
        worktree_created,
    })
}

fn start_github_background_task(
    target: &GithubActionTarget,
    reference: &str,
    post: bool,
    job: Option<&str>,
    commit: bool,
    no_run: bool,
    id: Option<String>,
    cwd: Option<String>,
) -> AppResult<String> {
    crate::cli::commands::task::start_task_json(TaskStartArgs {
        task: github_background_task_prompt(target, reference, post, job, commit),
        cwd,
        id,
        no_run,
        json: true,
        ..TaskStartArgs::default()
    })
}

fn github_background_task_prompt(
    target: &GithubActionTarget,
    reference: &str,
    post: bool,
    job: Option<&str>,
    commit: bool,
) -> String {
    let mut lines = vec![
        format!("Handle GitHub PR request {reference} in this isolated task worktree."),
        format!("- Event: {}", target.event_name),
        format!("- Repository: {}", target.repo),
        format!("- Pull request: #{}", target.number),
        format!("- Mode: {}", github_action_mode_label(target.mode)),
        format!("- Trigger matched: {}", target.trigger_matched),
        format!("- Reason: {}", target.reason),
        format!("- Post requested: {post}"),
        format!("- Commit requested: {commit}"),
    ];
    if let Some(job) = job.map(str::trim).filter(|job| !job.is_empty()) {
        lines.push(format!("- CI job focus: {job}"));
    }
    lines.push(String::new());
    match target.mode {
        GithubActionMode::Review => lines.push(
            "Review the PR from the current checkout, prioritize concrete findings, and leave a concise review summary.".to_string(),
        ),
        GithubActionMode::Fix => lines.push(
            "Fix the requested failure or CI issue, run focused validation, and leave the resulting diff in the task worktree.".to_string(),
        ),
        GithubActionMode::Patch => lines.push(
            "Prepare the requested patch, run focused validation, and leave the resulting diff in the task worktree.".to_string(),
        ),
        GithubActionMode::Auto => lines.push(
            "Resolve the requested PR action from the event context, run focused validation, and leave outputs in the task worktree.".to_string(),
        ),
    }
    lines.push(
        "Use the current checkout as the PR head/workspace if it is already checked out."
            .to_string(),
    );
    lines.push(
        "Do not push, merge, or mutate the remote; operators can inspect with `deepseek task diff` and finish with `deepseek task merge` or `deepseek task reject`.".to_string(),
    );
    lines.join("\n")
}

fn fixture_issue_comment_event(body: &str) -> String {
    format!(
        r#"{{
            "repository": {{"full_name": "owner/repo"}},
            "issue": {{"number": 11, "pull_request": {{"url": "https://api.github.com/repos/owner/repo/pulls/11"}}}},
            "comment": {{"body": "{}"}}
        }}"#,
        json_escape(body)
    )
}

fn run_fixture_checkout_commit_push(workdir: &Path, head_ref: &str) -> AppResult<String> {
    let remote = workdir.join("remote.git");
    let repo = workdir.join("repo");
    let remote_arg = remote.display().to_string();
    let repo_arg = repo.display().to_string();
    run_git(workdir, &["init", "--bare", &remote_arg])?;
    run_git(workdir, &["init", &repo_arg])?;
    run_git(&repo, &["config", "user.name", "deepseek-code-fixture"])?;
    run_git(
        &repo,
        &[
            "config",
            "user.email",
            "deepseek-code-fixture@example.invalid",
        ],
    )?;
    fs::write(repo.join("README.md"), "DeepSeekCode GitHub fixture\n")?;
    run_git(&repo, &["add", "README.md"])?;
    run_git(&repo, &["commit", "-m", "initial fixture commit"])?;
    run_git(&repo, &["branch", "-M", "main"])?;
    run_git(&repo, &["remote", "add", "origin", &remote_arg])?;
    run_git(&repo, &["checkout", "-b", head_ref])?;
    fs::write(repo.join("fixture.txt"), "before\n")?;
    run_git(&repo, &["add", "fixture.txt"])?;
    run_git(&repo, &["commit", "-m", "fixture branch baseline"])?;
    run_git(&repo, &["push", "origin", &format!("HEAD:{head_ref}")])?;

    run_git(&repo, &["checkout", head_ref])?;
    fs::write(repo.join("fixture.txt"), "after deepseek fixture write\n")?;
    run_git(&repo, &["add", "fixture.txt"])?;
    run_git(&repo, &["commit", "-m", "deepseek fixture write"])?;
    run_git(&repo, &["push", "origin", &format!("HEAD:{head_ref}")])?;

    let pushed_head = run_git(&repo, &["rev-parse", "HEAD"])?;
    let remote_head = run_git(
        workdir,
        &[
            "--git-dir",
            &remote_arg,
            "rev-parse",
            &format!("refs/heads/{head_ref}"),
        ],
    )?;
    if pushed_head.trim() != remote_head.trim() {
        return Err(app_error(format!(
            "GitHub fixture smoke push verification failed: local {} != remote {}",
            pushed_head.trim(),
            remote_head.trim()
        )));
    }
    Ok(pushed_head.trim().to_string())
}

fn run_git(cwd: &Path, args: &[&str]) -> AppResult<String> {
    let output = Command::new("git")
        .current_dir(cwd)
        .args(args)
        .output()
        .map_err(|error| app_error(format!("failed to run git {}: {error}", args.join(" "))))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let detail = if !stderr.is_empty() { stderr } else { stdout };
        return Err(app_error(format!(
            "`git {}` failed in {}: {detail}",
            args.join(" "),
            cwd.display()
        )));
    }
    String::from_utf8(output.stdout).map_err(|error| {
        app_error(format!(
            "git {} returned non-UTF-8 output: {error}",
            args.join(" ")
        ))
    })
}

fn github_fixture_temp_root() -> AppResult<PathBuf> {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| app_error(format!("system clock error: {error}")))?
        .as_nanos();
    Ok(std::env::temp_dir().join(format!(
        "deepseek-github-action-fixture-{}-{nanos}",
        std::process::id()
    )))
}

fn path_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn fetch_pr_head_json(reference: &str) -> AppResult<String> {
    let selector = gh_pr_view_selector(reference);
    let mut command = Command::new("gh");
    command.args(["pr", "view", &selector.reference_arg]);
    let command_label = if let Some(repo) = selector.repo.as_deref() {
        command.args(["--repo", repo]);
        format!(
            "gh pr view {} --repo {repo} --json headRepositoryOwner,headRefName",
            selector.reference_arg
        )
    } else {
        format!(
            "gh pr view {} --json headRepositoryOwner,headRefName",
            selector.reference_arg
        )
    };
    let output = command
        .args(["--json", "headRepositoryOwner,headRefName"])
        .output()
        .map_err(|error| app_error(format!("failed to run `{command_label}`: {error}")))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let detail = if stderr.is_empty() {
            format!("status {}", output.status)
        } else {
            stderr
        };
        return Err(app_error(format!("`{command_label}` failed: {detail}")));
    }
    String::from_utf8(output.stdout)
        .map_err(|error| app_error(format!("gh pr view returned non-UTF-8 JSON: {error}")))
}

fn gh_pr_view_selector(reference: &str) -> GhPrViewSelector {
    if let Some((repo, number)) = reference.split_once('#') {
        if !repo.is_empty() && !number.is_empty() && number.chars().all(|ch| ch.is_ascii_digit()) {
            return GhPrViewSelector {
                reference_arg: number.to_string(),
                repo: Some(repo.to_string()),
            };
        }
    }
    GhPrViewSelector {
        reference_arg: reference.to_string(),
        repo: None,
    }
}

fn parse_pr_head_target(reference: &str, body: &str) -> AppResult<GithubPrHeadTarget> {
    let root = parse_root_object(body)?;
    let head_owner = object_field(&root, "headRepositoryOwner")?;
    Ok(GithubPrHeadTarget {
        reference: reference.to_string(),
        head_owner: string_field(head_owner, "login")?.to_string(),
        head_ref: string_field(&root, "headRefName")?.to_string(),
    })
}

fn validate_pr_head_owner(target: &GithubPrHeadTarget, repo_owner: &str) -> AppResult<()> {
    let expected = repo_owner.trim();
    if expected.is_empty() {
        return Err(app_error("github pr-head --repo-owner cannot be empty"));
    }
    if target.head_owner != expected {
        return Err(app_error(format!(
            "refusing write workflow for fork-owned PR branch {}; expected {expected}",
            target.head_owner
        )));
    }
    Ok(())
}

fn validate_required_modes(
    target: &GithubActionTarget,
    required_modes: &[GithubActionMode],
) -> AppResult<()> {
    if required_modes.is_empty() || required_modes.iter().any(|mode| mode == &target.mode) {
        return Ok(());
    }
    let allowed = required_modes
        .iter()
        .map(|mode| github_action_mode_label(*mode))
        .collect::<Vec<_>>()
        .join("|");
    Err(app_error(format!(
        "GitHub action resolved mode `{}` but requires one of {allowed}",
        github_action_mode_label(target.mode)
    )))
}

fn write_github_output(target: &GithubActionTarget, post: bool) -> AppResult<()> {
    append_github_output(&render_github_output(target, post))
}

fn write_pr_head_github_output(target: &GithubPrHeadTarget) -> AppResult<()> {
    append_github_output(&render_pr_head_github_output(target))
}

fn append_github_output(content: &str) -> AppResult<()> {
    let output_path = std::env::var("GITHUB_OUTPUT")
        .map_err(|_| app_error("--github-output requires GITHUB_OUTPUT to be set"))?;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&output_path)
        .map_err(|error| {
            app_error(format!(
                "failed to open GITHUB_OUTPUT {output_path}: {error}"
            ))
        })?;
    file.write_all(content.as_bytes()).map_err(|error| {
        app_error(format!(
            "failed to write GITHUB_OUTPUT {output_path}: {error}"
        ))
    })
}

fn github_action_target_from_event(
    event_name: &str,
    body: &str,
    requested_mode: GithubActionMode,
    trigger: &str,
    allow_untriggered: bool,
) -> AppResult<GithubActionTarget> {
    let root = parse_root_object(body)?;
    let repo = repository_full_name(&root)?;
    let event_name = event_name.trim();
    match event_name {
        "pull_request" | "pull_request_target" => {
            let pull_request = object_field(&root, "pull_request")?;
            let number = u64_field(pull_request, "number").or_else(|_| u64_field(&root, "number"))?;
            Ok(GithubActionTarget {
                event_name: event_name.to_string(),
                repo,
                number,
                mode: resolve_action_mode(requested_mode, ""),
                trigger_matched: true,
                reason: "pull_request events review without comment trigger".to_string(),
            })
        }
        "issue_comment" => {
            let issue = object_field(&root, "issue")?;
            if !issue.contains_key("pull_request") {
                return Err(app_error(
                    "issue_comment event is not attached to a pull request",
                ));
            }
            let comment_body = object_field(&root, "comment")
                .ok()
                .and_then(|comment| string_field(comment, "body").ok())
                .unwrap_or("");
            let matched = trigger_matches(comment_body, trigger, allow_untriggered)?;
            Ok(GithubActionTarget {
                event_name: event_name.to_string(),
                repo,
                number: u64_field(issue, "number")?,
                mode: resolve_action_mode(requested_mode, command_after_trigger(comment_body, trigger)),
                trigger_matched: matched,
                reason: "issue_comment trigger matched on pull request".to_string(),
            })
        }
        "pull_request_review_comment" => {
            let pull_request = object_field(&root, "pull_request")?;
            let comment_body = object_field(&root, "comment")
                .ok()
                .and_then(|comment| string_field(comment, "body").ok())
                .unwrap_or("");
            let matched = trigger_matches(comment_body, trigger, allow_untriggered)?;
            Ok(GithubActionTarget {
                event_name: event_name.to_string(),
                repo,
                number: u64_field(pull_request, "number")?,
                mode: resolve_action_mode(requested_mode, command_after_trigger(comment_body, trigger)),
                trigger_matched: matched,
                reason: "pull_request_review_comment trigger matched".to_string(),
            })
        }
        "pull_request_review" => {
            let pull_request = object_field(&root, "pull_request")?;
            let review_body = object_field(&root, "review")
                .ok()
                .and_then(|review| string_field(review, "body").ok())
                .unwrap_or("");
            let matched = trigger_matches(review_body, trigger, allow_untriggered)?;
            Ok(GithubActionTarget {
                event_name: event_name.to_string(),
                repo,
                number: u64_field(pull_request, "number")?,
                mode: resolve_action_mode(requested_mode, command_after_trigger(review_body, trigger)),
                trigger_matched: matched,
                reason: "pull_request_review trigger matched".to_string(),
            })
        }
        other => Err(app_error(format!(
            "unsupported GitHub event `{other}`; expected pull_request|pull_request_target|issue_comment|pull_request_review|pull_request_review_comment"
        ))),
    }
}

fn resolve_action_mode(requested: GithubActionMode, command: &str) -> GithubActionMode {
    if !matches!(requested, GithubActionMode::Auto) {
        return requested;
    }
    match command
        .split_whitespace()
        .next()
        .unwrap_or("")
        .trim_matches(|ch: char| !ch.is_ascii_alphanumeric())
        .to_ascii_lowercase()
        .as_str()
    {
        "fix" | "repair" => GithubActionMode::Fix,
        "patch" | "apply" => GithubActionMode::Patch,
        "review" | "" => GithubActionMode::Review,
        _ => GithubActionMode::Review,
    }
}

fn command_after_trigger<'a>(body: &'a str, trigger: &str) -> &'a str {
    let trigger = trigger.trim();
    if trigger.is_empty() {
        return body;
    }
    let body_lower = body.to_lowercase();
    let trigger_lower = trigger.to_lowercase();
    let Some(index) = body_lower.find(&trigger_lower) else {
        return "";
    };
    body.get(index + trigger.len()..).unwrap_or("").trim()
}

fn repository_full_name(root: &std::collections::BTreeMap<String, JsonValue>) -> AppResult<String> {
    let repository = object_field(root, "repository")?;
    string_field(repository, "full_name").map(str::to_string)
}

fn trigger_matches(body: &str, trigger: &str, allow_untriggered: bool) -> AppResult<bool> {
    if allow_untriggered {
        return Ok(false);
    }
    let trigger = trigger.trim();
    if !body.to_lowercase().contains(&trigger.to_lowercase()) {
        return Err(app_error(format!(
            "GitHub event comment did not contain trigger `{trigger}`; pass --allow-untriggered to override"
        )));
    }
    Ok(true)
}

fn object_field<'a>(
    map: &'a std::collections::BTreeMap<String, JsonValue>,
    key: &str,
) -> AppResult<&'a std::collections::BTreeMap<String, JsonValue>> {
    map.get(key)
        .and_then(json_as_object)
        .ok_or_else(|| app_error(format!("GitHub event missing object `{key}`")))
}

fn string_field<'a>(
    map: &'a std::collections::BTreeMap<String, JsonValue>,
    key: &str,
) -> AppResult<&'a str> {
    map.get(key)
        .and_then(json_as_string)
        .ok_or_else(|| app_error(format!("GitHub event missing string `{key}`")))
}

fn u64_field(map: &std::collections::BTreeMap<String, JsonValue>, key: &str) -> AppResult<u64> {
    map.get(key)
        .and_then(json_as_u64)
        .ok_or_else(|| app_error(format!("GitHub event missing numeric `{key}`")))
}

fn render_action_target_json(target: &GithubActionTarget, post: bool) -> String {
    format!(
        "{{\"kind\":\"deepseek.github_action_target.v1\",\"event\":\"{}\",\"repo\":\"{}\",\"number\":{},\"reference\":\"{}#{}\",\"mode\":\"{}\",\"post\":{},\"trigger_matched\":{},\"reason\":\"{}\"}}",
        json_escape(&target.event_name),
        json_escape(&target.repo),
        target.number,
        json_escape(&target.repo),
        target.number,
        github_action_mode_label(target.mode),
        post,
        target.trigger_matched,
        json_escape(&target.reason)
    )
}

fn render_pr_head_json(target: &GithubPrHeadTarget) -> String {
    format!(
        "{{\"kind\":\"deepseek.github_pr_head.v1\",\"reference\":\"{}\",\"head_owner\":\"{}\",\"head_ref\":\"{}\"}}",
        json_escape(&target.reference),
        json_escape(&target.head_owner),
        json_escape(&target.head_ref)
    )
}

fn render_fixture_smoke_json(report: &GithubFixtureSmokeReport) -> String {
    let review = match &report.review {
        Some(review) => format!(
            "{{\"reference\":\"{}\",\"mode\":\"{}\",\"trigger_matched\":{}}}",
            json_escape(&review.reference),
            github_action_mode_label(review.mode),
            review.trigger_matched
        ),
        None => "null".to_string(),
    };
    let write = match &report.write {
        Some(write) => format!(
            "{{\"reference\":\"{}\",\"mode\":\"{}\",\"head_ref\":\"{}\",\"fork_guard_verified\":{},\"pushed_head\":\"{}\"}}",
            json_escape(&write.reference),
            github_action_mode_label(write.mode),
            json_escape(&write.head_ref),
            write.fork_guard_verified,
            json_escape(&write.pushed_head)
        ),
        None => "null".to_string(),
    };
    let background_task = match &report.background_task {
        Some(background_task) => format!(
            "{{\"reference\":\"{}\",\"mode\":\"{}\",\"task_id\":\"{}\",\"task_record_created\":{},\"worktree_created\":{}}}",
            json_escape(&background_task.reference),
            github_action_mode_label(background_task.mode),
            json_escape(&background_task.task_id),
            background_task.task_record_created,
            background_task.worktree_created
        ),
        None => "null".to_string(),
    };
    format!(
        "{{\"kind\":\"deepseek.github_action_fixture_smoke.v1\",\"workdir\":\"{}\",\"cleanup\":{},\"review\":{},\"write\":{},\"background_task\":{}}}",
        json_escape(&report.workdir.display().to_string()),
        report.cleanup,
        review,
        write,
        background_task
    )
}

fn print_fixture_smoke_report(report: &GithubFixtureSmokeReport) {
    println!("DeepSeekCode GitHub Action fixture smoke");
    println!("workdir: {}", report.workdir.display());
    println!(
        "cleanup: {}",
        if report.cleanup { "removed" } else { "kept" }
    );
    if let Some(review) = &report.review {
        println!(
            "review: pass reference={} mode={} trigger_matched={}",
            review.reference,
            github_action_mode_label(review.mode),
            review.trigger_matched
        );
    }
    if let Some(write) = &report.write {
        println!(
            "write: pass reference={} mode={} head_ref={} fork_guard={} pushed_head={}",
            write.reference,
            github_action_mode_label(write.mode),
            write.head_ref,
            write.fork_guard_verified,
            write.pushed_head
        );
    }
    if let Some(background_task) = &report.background_task {
        println!(
            "background-task: pass reference={} mode={} task_id={} record={} worktree={}",
            background_task.reference,
            github_action_mode_label(background_task.mode),
            background_task.task_id,
            background_task.task_record_created,
            background_task.worktree_created
        );
    }
}

fn render_github_output(target: &GithubActionTarget, post: bool) -> String {
    format!(
        "event={}\nrepo={}\nnumber={}\nreference={}#{}\nmode={}\npost={}\ntrigger_matched={}\n",
        target.event_name,
        target.repo,
        target.number,
        target.repo,
        target.number,
        github_action_mode_label(target.mode),
        post,
        target.trigger_matched
    )
}

fn render_pr_head_github_output(target: &GithubPrHeadTarget) -> String {
    format!(
        "reference={}\nhead_owner={}\nhead_ref={}\n",
        target.reference, target.head_owner, target.head_ref
    )
}

fn github_action_mode_label(mode: GithubActionMode) -> &'static str {
    match mode {
        GithubActionMode::Auto => "auto",
        GithubActionMode::Review => "review",
        GithubActionMode::Fix => "fix",
        GithubActionMode::Patch => "patch",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pull_request_event() -> &'static str {
        r#"{
            "number": 7,
            "repository": {"full_name": "owner/repo"},
            "pull_request": {"number": 7}
        }"#
    }

    fn issue_comment_event(body: &str) -> String {
        format!(
            r#"{{
                "repository": {{"full_name": "owner/repo"}},
                "issue": {{"number": 11, "pull_request": {{"url": "https://api.github.com/repos/owner/repo/pulls/11"}}}},
                "comment": {{"body": "{}"}}
            }}"#,
            json_escape(body)
        )
    }

    #[test]
    fn pull_request_event_maps_to_repo_pr_without_trigger() {
        let target = github_action_target_from_event(
            "pull_request",
            pull_request_event(),
            GithubActionMode::Auto,
            "@deepseek",
            false,
        )
        .unwrap();

        assert_eq!(target.repo, "owner/repo");
        assert_eq!(target.number, 7);
        assert_eq!(target.mode, GithubActionMode::Review);
        assert!(target.trigger_matched);
    }

    #[test]
    fn issue_comment_requires_pull_request_issue_and_trigger() {
        let target = github_action_target_from_event(
            "issue_comment",
            &issue_comment_event("@deepseek please review"),
            GithubActionMode::Auto,
            "@deepseek",
            false,
        )
        .unwrap();

        assert_eq!(target.repo, "owner/repo");
        assert_eq!(target.number, 11);
        assert_eq!(target.mode, GithubActionMode::Review);
        assert!(target.trigger_matched);
    }

    #[test]
    fn issue_comment_auto_mode_routes_fix_and_patch_commands() {
        let fix = github_action_target_from_event(
            "issue_comment",
            &issue_comment_event("@deepseek fix the failing CI"),
            GithubActionMode::Auto,
            "@deepseek",
            false,
        )
        .unwrap();
        let patch = github_action_target_from_event(
            "issue_comment",
            &issue_comment_event("@deepseek patch this follow-up"),
            GithubActionMode::Auto,
            "@deepseek",
            false,
        )
        .unwrap();

        assert_eq!(fix.mode, GithubActionMode::Fix);
        assert_eq!(patch.mode, GithubActionMode::Patch);
    }

    #[test]
    fn explicit_mode_overrides_comment_command() {
        let target = github_action_target_from_event(
            "issue_comment",
            &issue_comment_event("@deepseek fix the failing CI"),
            GithubActionMode::Review,
            "@deepseek",
            false,
        )
        .unwrap();

        assert_eq!(target.mode, GithubActionMode::Review);
    }

    #[test]
    fn issue_comment_rejects_missing_trigger_by_default() {
        let err = github_action_target_from_event(
            "issue_comment",
            &issue_comment_event("please review"),
            GithubActionMode::Auto,
            "@deepseek",
            false,
        )
        .unwrap_err();

        assert!(err.to_string().contains("did not contain trigger"));
    }

    #[test]
    fn issue_comment_can_allow_untriggered() {
        let target = github_action_target_from_event(
            "issue_comment",
            &issue_comment_event("please review"),
            GithubActionMode::Auto,
            "@deepseek",
            true,
        )
        .unwrap();

        assert_eq!(target.number, 11);
        assert_eq!(target.mode, GithubActionMode::Review);
        assert!(!target.trigger_matched);
    }

    #[test]
    fn issue_comment_rejects_plain_issue() {
        let body = r#"{
            "repository": {"full_name": "owner/repo"},
            "issue": {"number": 12},
            "comment": {"body": "@deepseek"}
        }"#;

        assert!(github_action_target_from_event(
            "issue_comment",
            body,
            GithubActionMode::Auto,
            "@deepseek",
            false
        )
        .is_err());
    }

    #[test]
    fn dry_run_json_renders_reference_and_post_flag() {
        let target = GithubActionTarget {
            event_name: "pull_request".to_string(),
            repo: "owner/repo".to_string(),
            number: 7,
            mode: GithubActionMode::Patch,
            trigger_matched: true,
            reason: "ok".to_string(),
        };
        let rendered = render_action_target_json(&target, true);

        assert!(rendered.contains("\"reference\":\"owner/repo#7\""));
        assert!(rendered.contains("\"mode\":\"patch\""));
        assert!(rendered.contains("\"post\":true"));
    }

    #[test]
    fn required_modes_reject_unexpected_mode() {
        let target = GithubActionTarget {
            event_name: "issue_comment".to_string(),
            repo: "owner/repo".to_string(),
            number: 7,
            mode: GithubActionMode::Review,
            trigger_matched: true,
            reason: "ok".to_string(),
        };

        let err =
            validate_required_modes(&target, &[GithubActionMode::Fix, GithubActionMode::Patch])
                .unwrap_err();
        assert!(err.to_string().contains("requires one of fix|patch"));
    }

    #[test]
    fn required_modes_accept_matching_mode() {
        let target = GithubActionTarget {
            event_name: "issue_comment".to_string(),
            repo: "owner/repo".to_string(),
            number: 7,
            mode: GithubActionMode::Patch,
            trigger_matched: true,
            reason: "ok".to_string(),
        };

        validate_required_modes(&target, &[GithubActionMode::Fix, GithubActionMode::Patch])
            .unwrap();
    }

    #[test]
    fn github_output_renders_action_step_fields() {
        let target = GithubActionTarget {
            event_name: "issue_comment".to_string(),
            repo: "owner/repo".to_string(),
            number: 11,
            mode: GithubActionMode::Fix,
            trigger_matched: true,
            reason: "ok".to_string(),
        };

        let rendered = render_github_output(&target, false);
        assert!(rendered.contains("reference=owner/repo#11\n"));
        assert!(rendered.contains("number=11\n"));
        assert!(rendered.contains("mode=fix\n"));
        assert!(rendered.contains("trigger_matched=true\n"));
    }

    #[test]
    fn pr_head_target_reads_owner_and_ref() {
        let target = parse_pr_head_target(
            "owner/repo#11",
            r#"{"headRepositoryOwner":{"login":"owner"},"headRefName":"feature/deepseek"}"#,
        )
        .unwrap();

        assert_eq!(target.reference, "owner/repo#11");
        assert_eq!(target.head_owner, "owner");
        assert_eq!(target.head_ref, "feature/deepseek");
    }

    #[test]
    fn pr_head_selector_converts_repo_hash_reference_for_gh() {
        let selector = gh_pr_view_selector("owner/repo#11");

        assert_eq!(selector.reference_arg, "11");
        assert_eq!(selector.repo.as_deref(), Some("owner/repo"));
    }

    #[test]
    fn pr_head_owner_guard_rejects_fork_owner() {
        let target = GithubPrHeadTarget {
            reference: "owner/repo#11".to_string(),
            head_owner: "fork".to_string(),
            head_ref: "feature/deepseek".to_string(),
        };

        let err = validate_pr_head_owner(&target, "owner").unwrap_err();
        assert!(err
            .to_string()
            .contains("refusing write workflow for fork-owned PR branch fork; expected owner"));
    }

    #[test]
    fn pr_head_json_renders_reference_owner_and_ref() {
        let target = GithubPrHeadTarget {
            reference: "owner/repo#11".to_string(),
            head_owner: "owner".to_string(),
            head_ref: "feature/deepseek".to_string(),
        };

        let rendered = render_pr_head_json(&target);
        assert!(rendered.contains("\"kind\":\"deepseek.github_pr_head.v1\""));
        assert!(rendered.contains("\"reference\":\"owner/repo#11\""));
        assert!(rendered.contains("\"head_owner\":\"owner\""));
        assert!(rendered.contains("\"head_ref\":\"feature/deepseek\""));
    }

    #[test]
    fn pr_head_github_output_renders_head_ref() {
        let target = GithubPrHeadTarget {
            reference: "owner/repo#11".to_string(),
            head_owner: "owner".to_string(),
            head_ref: "feature/deepseek".to_string(),
        };

        let rendered = render_pr_head_github_output(&target);
        assert!(rendered.contains("reference=owner/repo#11\n"));
        assert!(rendered.contains("head_owner=owner\n"));
        assert!(rendered.contains("head_ref=feature/deepseek\n"));
    }

    #[test]
    fn fixture_review_smoke_resolves_review_target() {
        let review = run_fixture_review_smoke().unwrap();

        assert_eq!(review.reference, "owner/repo#11");
        assert_eq!(review.mode, GithubActionMode::Review);
        assert!(review.trigger_matched);
    }

    #[test]
    fn fixture_smoke_json_reports_write_push_and_guard() {
        let report = GithubFixtureSmokeReport {
            workdir: PathBuf::from("/tmp/deepseek-github-action-fixture"),
            cleanup: true,
            review: Some(GithubFixtureReviewSmoke {
                reference: "owner/repo#11".to_string(),
                mode: GithubActionMode::Review,
                trigger_matched: true,
            }),
            write: Some(GithubFixtureWriteSmoke {
                reference: "owner/repo#11".to_string(),
                mode: GithubActionMode::Fix,
                head_ref: "feature/deepseek".to_string(),
                fork_guard_verified: true,
                pushed_head: "abc123".to_string(),
            }),
            background_task: Some(GithubFixtureBackgroundTaskSmoke {
                reference: "owner/repo#11".to_string(),
                mode: GithubActionMode::Fix,
                task_id: "github-bg-smoke".to_string(),
                task_record_created: true,
                worktree_created: true,
            }),
        };

        let rendered = render_fixture_smoke_json(&report);
        assert!(rendered.contains("\"kind\":\"deepseek.github_action_fixture_smoke.v1\""));
        assert!(rendered.contains("\"mode\":\"review\""));
        assert!(rendered.contains("\"mode\":\"fix\""));
        assert!(rendered.contains("\"fork_guard_verified\":true"));
        assert!(rendered.contains("\"pushed_head\":\"abc123\""));
        assert!(rendered.contains("\"background_task\":"));
        assert!(rendered.contains("\"task_id\":\"github-bg-smoke\""));
        assert!(rendered.contains("\"task_record_created\":true"));
    }

    #[test]
    fn github_background_task_prompt_includes_action_context() {
        let target = GithubActionTarget {
            event_name: "issue_comment".to_string(),
            repo: "owner/repo".to_string(),
            number: 11,
            mode: GithubActionMode::Fix,
            trigger_matched: true,
            reason: "issue_comment trigger matched on pull request".to_string(),
        };

        let prompt =
            github_background_task_prompt(&target, "owner/repo#11", false, Some("test-ci"), false);

        assert!(prompt.contains("owner/repo#11"));
        assert!(prompt.contains("Mode: fix"));
        assert!(prompt.contains("CI job focus: test-ci"));
        assert!(prompt.contains("isolated task worktree"));
        assert!(prompt.contains("Do not push"));
    }
}
