use crate::cli::app::{BenchmarkArgs, PrAction};
use crate::config::load::load_or_default;
use crate::config::types::AppConfig;
use crate::core::context::TaskContext;
use crate::core::loop_runtime::{AgentLoop, AgentLoopOptions};
use crate::error::{app_error, AppResult};
use crate::integrations::github::{
    current_branch, ensure_gh_auth, fetch_first_failed_job, fetch_pr, fetch_repo_permissions,
    parse_pr_ref, post_pr_comment, require_on_branch, worktree_is_clean, CiFailure, PrContext,
    RepoPermissions,
};
use crate::model::protocol::Observation;
use crate::util::json::json_escape;
use std::path::{Component, Path, PathBuf};

pub fn run(action: PrAction) -> AppResult<()> {
    match action {
        PrAction::LiveStatus {
            reference,
            require_write,
            json,
        } => run_live_status(&reference, require_write, json),
        action => {
            let config = load_or_default()?;
            warn_if_offline_planner(&config);
            run_model_backed_action(config, action)
        }
    }
}

fn run_model_backed_action(config: AppConfig, action: PrAction) -> AppResult<()> {
    match action {
        PrAction::Review {
            reference,
            post,
            out,
        } => run_review(config, &reference, post, out.as_deref()),
        PrAction::Fix {
            reference,
            job,
            request,
            benchmark_gate,
        } => run_fix(
            config,
            &reference,
            job.as_deref(),
            request.as_deref(),
            benchmark_gate,
        ),
        PrAction::Patch {
            reference,
            request,
            commit,
            benchmark_gate,
        } => run_patch(
            config,
            &reference,
            request.as_deref(),
            commit,
            benchmark_gate,
        ),
        PrAction::LiveStatus { .. } => unreachable!("handled before loading model config"),
    }
}

fn run_live_status(reference: &str, require_write: bool, json: bool) -> AppResult<()> {
    ensure_gh_auth()?;
    let pr_ref = parse_pr_ref(reference)?;
    let pr = fetch_pr(&pr_ref)?;
    let permissions = fetch_repo_permissions(&pr.repo)?;
    let report = build_live_status_report(&pr, &permissions, current_branch(), require_write);

    if json {
        println!("{}", render_live_status_json(&pr, &report, require_write));
    } else {
        println!("DeepSeekCode PR live status");
        println!("  target: {}#{}", pr.repo, pr.number);
        println!("  title: {}", pr.title);
        println!("  branch: {}", pr.branch);
        println!("  changed_files: {}", pr.changed_files.len());
        println!("  diff_bytes: {}", pr.diff.len());
        for check in &report.checks {
            println!(
                "  {}: {} ({})",
                check.name,
                check.status.label(),
                check.detail
            );
        }
        println!("  not_ready: {}", report.not_ready_count());
        if report.not_ready_count() == 0 {
            println!("  next: live remote PR fixture prerequisites are available");
        } else {
            println!("  next: resolve blocked checks before running a write-capable live fixture");
        }
    }

    if require_write && report.not_ready_count() > 0 {
        return Err(app_error(format!(
            "PR live status is not ready: {} check(s) are blocked",
            report.not_ready_count()
        )));
    }
    Ok(())
}

fn render_live_status_json(
    pr: &PrContext,
    report: &PrLiveStatusReport,
    require_write: bool,
) -> String {
    let checks = report
        .checks
        .iter()
        .map(|check| {
            format!(
                "{{\"name\":\"{}\",\"status\":\"{}\",\"detail\":\"{}\"}}",
                json_escape(check.name),
                check.status.label(),
                json_escape(&check.detail)
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    let target = format!("{}#{}", pr.repo, pr.number);
    format!(
        "{{\"kind\":\"deepseek.pr_live_status.v1\",\"target\":\"{}\",\"repo\":\"{}\",\"number\":{},\"title\":\"{}\",\"branch\":\"{}\",\"changed_files\":{},\"diff_bytes\":{},\"require_write\":{},\"not_ready\":{},\"checks\":[{}]}}",
        json_escape(&target),
        json_escape(&pr.repo),
        pr.number,
        json_escape(&pr.title),
        json_escape(&pr.branch),
        pr.changed_files.len(),
        pr.diff.len(),
        require_write,
        report.not_ready_count(),
        checks
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PrLiveStatusReport {
    checks: Vec<PrLiveStatusCheck>,
}

impl PrLiveStatusReport {
    fn not_ready_count(&self) -> usize {
        self.checks
            .iter()
            .filter(|check| !check.status.is_ready())
            .count()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PrLiveStatusCheck {
    name: &'static str,
    status: PrLiveStatus,
    detail: String,
}

impl PrLiveStatusCheck {
    fn ready(name: &'static str, detail: impl Into<String>) -> Self {
        Self {
            name,
            status: PrLiveStatus::Ready,
            detail: detail.into(),
        }
    }

    fn blocked(name: &'static str, detail: impl Into<String>) -> Self {
        Self {
            name,
            status: PrLiveStatus::Blocked,
            detail: detail.into(),
        }
    }

    fn skipped(name: &'static str, detail: impl Into<String>) -> Self {
        Self {
            name,
            status: PrLiveStatus::Skipped,
            detail: detail.into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PrLiveStatus {
    Ready,
    Blocked,
    Skipped,
}

impl PrLiveStatus {
    fn label(self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::Blocked => "blocked",
            Self::Skipped => "skipped",
        }
    }

    fn is_ready(self) -> bool {
        !matches!(self, Self::Blocked)
    }
}

fn build_live_status_report(
    pr: &PrContext,
    permissions: &RepoPermissions,
    current_branch: Option<String>,
    require_write: bool,
) -> PrLiveStatusReport {
    let mut checks = Vec::new();
    checks.push(if pr.diff.trim().is_empty() {
        PrLiveStatusCheck::blocked(
            "pr_diff",
            "PR diff is empty or unavailable; remote review fixtures need include_diff context",
        )
    } else {
        PrLiveStatusCheck::ready(
            "pr_diff",
            format!("diff loaded ({} byte(s))", pr.diff.len()),
        )
    });
    checks.push(if pr.changed_files.is_empty() {
        PrLiveStatusCheck::blocked(
            "changed_files",
            "PR changed file list is empty; inline review fixtures need file paths",
        )
    } else {
        PrLiveStatusCheck::ready(
            "changed_files",
            format!("{} changed file(s) visible", pr.changed_files.len()),
        )
    });
    checks.push(match current_branch {
        Some(branch) if branch == pr.branch => {
            PrLiveStatusCheck::ready("branch", format!("current branch matches `{}`", pr.branch))
        }
        Some(branch) => PrLiveStatusCheck::skipped(
            "branch",
            format!(
                "current branch `{branch}` does not match PR head `{}`; read-only review is still possible",
                pr.branch
            ),
        ),
        None => PrLiveStatusCheck::skipped(
            "branch",
            "current git branch could not be determined; read-only review is still possible",
        ),
    });
    checks.push(if permissions.pull {
        PrLiveStatusCheck::ready(
            "repo_read",
            "authenticated user can read repository metadata",
        )
    } else {
        PrLiveStatusCheck::blocked(
            "repo_read",
            "repository permissions do not report pull access",
        )
    });
    if require_write {
        checks.push(if permissions.can_write_pr_comments() {
            PrLiveStatusCheck::ready(
                "repo_write",
                "repository permissions report push/maintain/admin access for write fixtures",
            )
        } else {
            PrLiveStatusCheck::blocked(
                "repo_write",
                "repository permissions do not report push/maintain/admin access; guarded GitHub comment fixtures may fail",
            )
        });
    } else {
        checks.push(PrLiveStatusCheck::skipped(
            "repo_write",
            "pass --require-write to require write-capable repository permissions",
        ));
    }

    PrLiveStatusReport { checks }
}

fn warn_if_offline_planner(config: &AppConfig) {
    let api_key_present = std::env::var(&config.model.api_key_env)
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false);
    if !api_key_present {
        eprintln!(
            "[offline] {} is not set; the offline planner will produce a shallow report. Export it for a real LLM-driven review.",
            config.model.api_key_env
        );
    }
}

fn run_review(config: AppConfig, reference: &str, post: bool, out: Option<&str>) -> AppResult<()> {
    ensure_gh_auth()?;
    let pr_ref = parse_pr_ref(reference)?;
    let pr = fetch_pr(&pr_ref)?;

    let task = build_review_task_text(&pr);
    let context = TaskContext::new(task, Some("pr-review".to_string()));

    let observations = vec![
        Observation::ok("git_diff", pr.diff.clone()),
        Observation::ok("list_files", pr.changed_files.join("\n")),
    ];

    let runtime = AgentLoop::new(config.clone());
    let result = runtime.run_with(
        context,
        AgentLoopOptions {
            steps: 6,
            initial_observations: observations,
            ..AgentLoopOptions::default()
        },
    )?;
    let final_message = result.final_message;

    let body = build_review_body(&pr, &final_message);
    deliver_review(&pr, &body, post, out)?;
    Ok(())
}

fn build_review_body(pr: &PrContext, planner_output: &str) -> String {
    let header = format!(
        "## DeepSeekCode review of PR #{} ({})\n\n",
        pr.number, pr.title
    );
    let trimmed = planner_output.trim();
    if trimmed.is_empty() {
        return format!(
            "{header}_The planner returned no review content. See the terminal trace for the full session._\n"
        );
    }
    format!("{header}{trimmed}\n")
}

fn build_review_task_text(pr: &PrContext) -> String {
    format!(
        "Review pull request #{} '{}' in repository {} on branch {}. Use the provided PR diff and changed-file observations first. Highlight correctness risks, security concerns, and style violations. Output a markdown report.",
        pr.number, pr.title, pr.repo, pr.branch
    )
}

fn deliver_review(pr: &PrContext, body: &str, post: bool, out: Option<&str>) -> AppResult<()> {
    if let Some(path) = out {
        std::fs::write(path, body)?;
        println!("review written to {path}");
    }
    if post {
        post_pr_comment(&pr.repo, pr.number, body)?;
        println!("review posted as comment on {}#{}", pr.repo, pr.number);
    }
    if !post && out.is_none() {
        println!("{body}");
    }
    Ok(())
}

fn run_fix(
    config: AppConfig,
    reference: &str,
    job_filter: Option<&str>,
    request: Option<&str>,
    benchmark_gate: bool,
) -> AppResult<()> {
    ensure_gh_auth()?;
    let pr_ref = parse_pr_ref(reference)?;
    let pr = fetch_pr(&pr_ref)?;
    require_on_branch(&pr.branch)?;

    let failure = match fetch_first_failed_job(&pr, job_filter)? {
        Some(failure) => failure,
        None => {
            println!("no failed CI jobs on PR #{}", pr.number);
            return Ok(());
        }
    };

    let task = build_fix_task_text(&pr, &failure, request);
    let context = TaskContext::new(task, None);
    let observations = vec![Observation::ok("run_shell", failure.log_tail.clone())];

    let runtime = AgentLoop::new(config.clone());
    runtime.run_with(
        context,
        AgentLoopOptions {
            steps: 12,
            initial_observations: observations,
            ..AgentLoopOptions::default()
        },
    )?;

    println!(
        "fix attempt complete for job `{}` (run #{}); review `git diff HEAD` and rerun if needed",
        failure.job_name, failure.run_id
    );
    if benchmark_gate {
        run_post_task_benchmark_gate(&config, &format!("pr fix #{}", pr.number))?;
    }
    Ok(())
}

fn build_fix_task_text(pr: &PrContext, failure: &CiFailure, request: Option<&str>) -> String {
    let step_clause = failure
        .failed_step
        .as_ref()
        .map(|step| format!(" at step `{step}`"))
        .unwrap_or_default();
    let mut text = format!(
        "CI job `{job}` (run #{run_id}) on PR #{number} failed{step_clause}. Reproduce locally, fix the root cause, and rerun the failing test. Failed log tail follows.",
        job = failure.job_name,
        run_id = failure.run_id,
        number = pr.number,
    );
    append_requested_action(&mut text, request);
    text
}

fn run_patch(
    config: AppConfig,
    reference: &str,
    request: Option<&str>,
    commit: bool,
    benchmark_gate: bool,
) -> AppResult<()> {
    ensure_gh_auth()?;
    let pr_ref = parse_pr_ref(reference)?;
    let pr = fetch_pr(&pr_ref)?;
    require_on_branch(&pr.branch)?;
    if commit && !worktree_is_clean()? {
        return Err(crate::error::policy_denied(
            "working tree has uncommitted changes; commit or stash before --commit",
        ));
    }

    if let Some(edit) = request.and_then(parse_direct_replacement_request) {
        let changed = apply_direct_replacement(&edit)?;
        if changed {
            println!("applied requested replacement in {}", edit.path.display());
        } else {
            println!(
                "requested replacement already present in {}",
                edit.path.display()
            );
        }
        finish_patch(&config, &pr, commit, benchmark_gate)?;
        return Ok(());
    }

    let task = build_patch_task_text(&pr, request);
    let context = TaskContext::new(task, None);
    let observations = vec![Observation::ok("git_diff", pr.diff.clone())];

    let runtime = AgentLoop::new(config.clone());
    runtime.run_with(
        context,
        AgentLoopOptions {
            steps: 8,
            initial_observations: observations,
            ..AgentLoopOptions::default()
        },
    )?;

    finish_patch(&config, &pr, commit, benchmark_gate)
}

fn finish_patch(
    config: &AppConfig,
    pr: &PrContext,
    commit: bool,
    benchmark_gate: bool,
) -> AppResult<()> {
    if commit {
        run_git(&["add", "-A"])?;
        let message = format!("deepseek: fix PR #{}", pr.number);
        run_git(&["commit", "-m", &message])?;
        println!("committed staged changes (no push)");
    } else {
        println!("changes left in worktree; run `git diff` to inspect, then commit manually");
    }
    if benchmark_gate {
        run_post_task_benchmark_gate(&config, &format!("pr patch #{}", pr.number))?;
    }
    Ok(())
}

fn build_patch_task_text(pr: &PrContext, request: Option<&str>) -> String {
    let mut text = format!(
        "Address review feedback or apply the requested change in PR #{} '{}' in repository {} on branch {}. Use the provided PR diff observation and the current checkout first; when the requested change is clear, edit files directly, then run focused validation.",
        pr.number, pr.title, pr.repo, pr.branch
    );
    append_requested_action(&mut text, request);
    text
}

fn append_requested_action(text: &mut String, request: Option<&str>) {
    if let Some(request) = request.map(str::trim).filter(|request| !request.is_empty()) {
        text.push_str("\n\nRequested action from GitHub comment: ");
        text.push_str(request);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DirectReplacementRequest {
    path: PathBuf,
    find: String,
    replace: String,
}

fn parse_direct_replacement_request(request: &str) -> Option<DirectReplacementRequest> {
    let request = request.trim();
    if request.is_empty() {
        return None;
    }
    let lower = request.to_ascii_lowercase();
    let quoted = quoted_segments(request);

    if lower.contains(" becomes ") && quoted.len() >= 3 {
        return Some(DirectReplacementRequest {
            path: safe_relative_path(&quoted[0]).ok()?,
            find: quoted[1].clone(),
            replace: quoted[2].clone(),
        });
    }

    if lower.contains("replace ")
        && lower.contains(" with ")
        && lower.contains(" in ")
        && quoted.len() >= 2
    {
        let in_index = lower.rfind(" in ")?;
        return Some(DirectReplacementRequest {
            path: safe_relative_path(request[in_index + 4..].trim().trim_matches('`')).ok()?,
            find: quoted[quoted.len() - 2].clone(),
            replace: quoted[quoted.len() - 1].clone(),
        });
    }

    None
}

fn quoted_segments(text: &str) -> Vec<String> {
    let mut segments = Vec::new();
    let mut index = 0;
    while index < text.len() {
        let Some((start_offset, quote)) = text[index..]
            .char_indices()
            .find(|(_, ch)| matches!(ch, '`' | '"'))
        else {
            break;
        };
        let start_quote = index + start_offset;
        let content_start = start_quote + quote.len_utf8();
        let Some(end_offset) = text[content_start..].find(quote) else {
            break;
        };
        let content_end = content_start + end_offset;
        segments.push(text[content_start..content_end].to_string());
        index = content_end + quote.len_utf8();
    }
    segments
}

fn safe_relative_path(path: &str) -> AppResult<PathBuf> {
    let path = Path::new(path.trim());
    if path.as_os_str().is_empty() || path.is_absolute() {
        return Err(app_error("direct patch request path must be relative"));
    }
    if path.components().any(|component| {
        matches!(
            component,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        )
    }) {
        return Err(app_error(
            "direct patch request path cannot escape the workspace",
        ));
    }
    Ok(path.to_path_buf())
}

fn apply_direct_replacement(edit: &DirectReplacementRequest) -> AppResult<bool> {
    let body = std::fs::read_to_string(&edit.path).map_err(|error| {
        app_error(format!(
            "failed to read direct patch target {}: {error}",
            edit.path.display()
        ))
    })?;
    if !body.contains(&edit.find) {
        if body.contains(&edit.replace) {
            return Ok(false);
        }
        return Err(app_error(format!(
            "direct patch target {} did not contain requested text",
            edit.path.display()
        )));
    }
    let updated = body.replacen(&edit.find, &edit.replace, 1);
    std::fs::write(&edit.path, updated).map_err(|error| {
        app_error(format!(
            "failed to write direct patch target {}: {error}",
            edit.path.display()
        ))
    })?;
    Ok(true)
}

fn run_git(args: &[&str]) -> AppResult<()> {
    crate::util::process::run_capture_stdout("git", args).map(|_| ())
}

fn run_post_task_benchmark_gate(config: &AppConfig, source: &str) -> AppResult<()> {
    println!("post-task benchmark gate ({source}): running default benchmark baseline");
    crate::cli::commands::benchmark::run_with_config(config.clone(), BenchmarkArgs::default())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_pr(number: u64, title: &str) -> PrContext {
        PrContext {
            number,
            repo: "owner/repo".to_string(),
            title: title.to_string(),
            branch: "feat/x".to_string(),
            base_branch: "main".to_string(),
            diff: String::new(),
            changed_files: Vec::new(),
        }
    }

    fn fixture_failure() -> CiFailure {
        CiFailure {
            run_id: 555,
            job_name: "test-rust".to_string(),
            job_id: 7,
            log_tail: "FAILED at line 42".to_string(),
            failed_step: Some("cargo test".to_string()),
        }
    }

    fn unique_pr_test_dir(label: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "deepseek-pr-test-{}-{nanos}",
            label.replace('/', "-")
        ))
    }

    struct CwdGuard(PathBuf);

    impl Drop for CwdGuard {
        fn drop(&mut self) {
            let _ = std::env::set_current_dir(&self.0);
        }
    }

    #[test]
    fn review_task_text_mentions_number_and_title() {
        let text = build_review_task_text(&fixture_pr(12, "Add feature X"));
        assert!(text.contains("#12"));
        assert!(text.contains("Add feature X"));
        assert!(text.contains("owner/repo"));
    }

    #[test]
    fn review_body_inlines_planner_output_when_present() {
        let pr = fixture_pr(7, "Tighten retry");
        let planner = "## Summary\n\nLooks good. One nit: ...";
        let body = build_review_body(&pr, planner);
        assert!(body.contains("PR #7"));
        assert!(body.contains("Tighten retry"));
        assert!(body.contains("## Summary"));
        assert!(body.contains("One nit"));
    }

    #[test]
    fn review_body_falls_back_when_planner_output_empty() {
        let pr = fixture_pr(7, "Empty");
        let body = build_review_body(&pr, "   \n  \n");
        assert!(body.contains("planner returned no review content"));
    }

    #[test]
    fn fix_task_text_includes_run_id_and_step() {
        let text = build_fix_task_text(&fixture_pr(12, "Some PR"), &fixture_failure(), None);
        assert!(text.contains("run #555"));
        assert!(text.contains("test-rust"));
        assert!(text.contains("cargo test"));
        assert!(text.contains("PR #12"));
    }

    #[test]
    fn patch_task_text_mentions_pr_number_and_title() {
        let text = build_patch_task_text(&fixture_pr(9, "Tighten retry loop"), None);
        assert!(text.contains("#9"));
        assert!(text.contains("Tighten retry loop"));
    }

    #[test]
    fn patch_task_text_includes_github_comment_request() {
        let text = build_patch_task_text(
            &fixture_pr(9, "Tighten retry loop"),
            Some("change docs/hosted-workflow-fixture.md to after"),
        );

        assert!(text.contains("Requested action from GitHub comment"));
        assert!(text.contains("change docs/hosted-workflow-fixture.md to after"));
    }

    #[test]
    fn parses_direct_replacement_from_comment_request() {
        let request = parse_direct_replacement_request(
            "change `docs/hosted-workflow-fixture.md` so `Current state: before` becomes `Current state: after`",
        )
        .unwrap();

        assert_eq!(
            request.path,
            PathBuf::from("docs/hosted-workflow-fixture.md")
        );
        assert_eq!(request.find, "Current state: before");
        assert_eq!(request.replace, "Current state: after");
    }

    #[test]
    fn apply_direct_replacement_updates_one_file() {
        let root = unique_pr_test_dir("direct-replacement");
        std::fs::create_dir_all(root.join("docs")).unwrap();
        let original_cwd = std::env::current_dir().unwrap();
        let guard = CwdGuard(original_cwd);
        std::env::set_current_dir(&root).unwrap();
        std::fs::write(
            root.join("docs/hosted-workflow-fixture.md"),
            "Requested state: after\nCurrent state: before\n",
        )
        .unwrap();

        let changed = apply_direct_replacement(&DirectReplacementRequest {
            path: PathBuf::from("docs/hosted-workflow-fixture.md"),
            find: "Current state: before".to_string(),
            replace: "Current state: after".to_string(),
        })
        .unwrap();

        assert!(changed);
        let body = std::fs::read_to_string(root.join("docs/hosted-workflow-fixture.md")).unwrap();
        assert!(body.contains("Current state: after"));
        drop(guard);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn live_status_reports_readiness_without_write_requirement() {
        let mut pr = fixture_pr(42, "Route benchmark command");
        pr.diff = "diff --git a/src/cli/app.rs b/src/cli/app.rs".to_string();
        pr.changed_files = vec!["src/cli/app.rs".to_string()];
        let report = build_live_status_report(
            &pr,
            &RepoPermissions {
                pull: true,
                push: false,
                maintain: false,
                admin: false,
            },
            Some("feature/other".to_string()),
            false,
        );

        assert_eq!(status_of(&report, "pr_diff"), PrLiveStatus::Ready);
        assert_eq!(status_of(&report, "changed_files"), PrLiveStatus::Ready);
        assert_eq!(status_of(&report, "branch"), PrLiveStatus::Skipped);
        assert_eq!(status_of(&report, "repo_read"), PrLiveStatus::Ready);
        assert_eq!(status_of(&report, "repo_write"), PrLiveStatus::Skipped);
        assert_eq!(report.not_ready_count(), 0);
    }

    #[test]
    fn live_status_blocks_when_write_required_without_repo_write_permission() {
        let mut pr = fixture_pr(42, "Route benchmark command");
        pr.diff = "diff --git a/src/cli/app.rs b/src/cli/app.rs".to_string();
        pr.changed_files = vec!["src/cli/app.rs".to_string()];
        let report = build_live_status_report(
            &pr,
            &RepoPermissions {
                pull: true,
                push: false,
                maintain: false,
                admin: false,
            },
            Some("feat/x".to_string()),
            true,
        );

        assert_eq!(status_of(&report, "branch"), PrLiveStatus::Ready);
        assert_eq!(status_of(&report, "repo_write"), PrLiveStatus::Blocked);
        assert_eq!(report.not_ready_count(), 1);
    }

    #[test]
    fn live_status_blocks_missing_diff_or_changed_files() {
        let pr = fixture_pr(42, "Empty diff");
        let report = build_live_status_report(
            &pr,
            &RepoPermissions {
                pull: true,
                push: true,
                maintain: false,
                admin: false,
            },
            Some("feat/x".to_string()),
            true,
        );

        assert_eq!(status_of(&report, "pr_diff"), PrLiveStatus::Blocked);
        assert_eq!(status_of(&report, "changed_files"), PrLiveStatus::Blocked);
        assert_eq!(status_of(&report, "repo_write"), PrLiveStatus::Ready);
        assert_eq!(report.not_ready_count(), 2);
    }

    #[test]
    fn render_live_status_json_includes_target_and_checks() {
        let mut pr = fixture_pr(42, "Quote \"ready\"");
        pr.diff = "diff --git a/src/main.rs b/src/main.rs\n+ok".to_string();
        pr.changed_files = vec!["src/main.rs".to_string()];
        let report = build_live_status_report(
            &pr,
            &RepoPermissions {
                pull: true,
                push: true,
                maintain: false,
                admin: false,
            },
            Some("feat/x".to_string()),
            true,
        );

        let json = render_live_status_json(&pr, &report, true);

        assert!(json.contains("\"kind\":\"deepseek.pr_live_status.v1\""));
        assert!(json.contains("\"target\":\"owner/repo#42\""));
        assert!(json.contains("\"title\":\"Quote \\\"ready\\\"\""));
        assert!(json.contains("\"require_write\":true"));
        assert!(json.contains("\"not_ready\":0"));
        assert!(json.contains("\"name\":\"repo_write\""));
        assert!(json.contains("\"status\":\"ready\""));
    }

    fn status_of(report: &PrLiveStatusReport, name: &str) -> PrLiveStatus {
        report
            .checks
            .iter()
            .find(|check| check.name == name)
            .unwrap_or_else(|| panic!("missing check {name}"))
            .status
    }
}
