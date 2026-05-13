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
            benchmark_gate,
        } => run_fix(config, &reference, job.as_deref(), benchmark_gate),
        PrAction::Patch {
            reference,
            commit,
            benchmark_gate,
        } => run_patch(config, &reference, commit, benchmark_gate),
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
            steps: 4,
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
        "Review pull request #{} '{}' on {}/{}. Highlight correctness risks, security concerns, and style violations. Output a markdown report.",
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

    let task = build_fix_task_text(&pr, &failure);
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

fn build_fix_task_text(pr: &PrContext, failure: &CiFailure) -> String {
    let step_clause = failure
        .failed_step
        .as_ref()
        .map(|step| format!(" at step `{step}`"))
        .unwrap_or_default();
    format!(
        "CI job `{job}` (run #{run_id}) on PR #{number} failed{step_clause}. Reproduce locally, fix the root cause, and rerun the failing test. Failed log tail follows.",
        job = failure.job_name,
        run_id = failure.run_id,
        number = pr.number,
    )
}

fn run_patch(
    config: AppConfig,
    reference: &str,
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

    let task = build_patch_task_text(&pr);
    let context = TaskContext::new(task, None);
    let observations = vec![Observation::ok("git_diff", pr.diff.clone())];

    let runtime = AgentLoop::new(config.clone());
    runtime.run_with(
        context,
        AgentLoopOptions {
            steps: 4,
            initial_observations: observations,
            ..AgentLoopOptions::default()
        },
    )?;

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

fn build_patch_task_text(pr: &PrContext) -> String {
    format!(
        "Address review feedback or apply the requested change in PR #{} '{}'. PR diff is the current head; propose minimal additional changes.",
        pr.number, pr.title
    )
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
        let text = build_fix_task_text(&fixture_pr(12, "Some PR"), &fixture_failure());
        assert!(text.contains("run #555"));
        assert!(text.contains("test-rust"));
        assert!(text.contains("cargo test"));
        assert!(text.contains("PR #12"));
    }

    #[test]
    fn patch_task_text_mentions_pr_number_and_title() {
        let text = build_patch_task_text(&fixture_pr(9, "Tighten retry loop"));
        assert!(text.contains("#9"));
        assert!(text.contains("Tighten retry loop"));
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
