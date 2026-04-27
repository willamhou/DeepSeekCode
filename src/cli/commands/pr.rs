use crate::cli::app::PrAction;
use crate::config::load::load_or_default;
use crate::core::agent::Agent;
use crate::core::context::TaskContext;
use crate::core::loop_runtime::AgentLoopOptions;
use crate::error::AppResult;
use crate::integrations::github::{
    ensure_gh_auth, fetch_pr, parse_pr_ref, post_pr_comment, PrContext,
};
use crate::model::protocol::Observation;

pub fn run(action: PrAction) -> AppResult<()> {
    match action {
        PrAction::Review { reference, post, out } => {
            run_review(&reference, post, out.as_deref())
        }
        PrAction::Fix { .. } => Err(crate::error::app_error(
            "pr fix is implemented in a later task",
        )),
        PrAction::Patch { .. } => Err(crate::error::app_error(
            "pr patch is implemented in a later task",
        )),
    }
}

fn run_review(reference: &str, post: bool, out: Option<&str>) -> AppResult<()> {
    ensure_gh_auth()?;
    let pr_ref = parse_pr_ref(reference)?;
    let pr = fetch_pr(&pr_ref)?;

    let task = build_review_task_text(&pr);
    let context = TaskContext::new(task, Some("pr-review".to_string()));

    let observations = vec![
        Observation::ok("git_diff", pr.diff.clone()),
        Observation::ok("list_files", pr.changed_files.join("\n")),
    ];

    let config = load_or_default()?;
    let mut agent = Agent::new(config);
    agent.run_with(
        context,
        AgentLoopOptions {
            steps: 4,
            initial_observations: observations,
        },
    )?;

    let body = format!(
        "DeepseekCode review of PR #{} ({})\n\nSee terminal trace above for the full review.",
        pr.number, pr.title
    );
    deliver_review(&pr, &body, post, out)?;
    Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_pr() -> PrContext {
        PrContext {
            number: 12,
            repo: "owner/repo".to_string(),
            title: "Add feature X".to_string(),
            branch: "feat/x".to_string(),
            base_branch: "main".to_string(),
            diff: String::new(),
            changed_files: Vec::new(),
        }
    }

    #[test]
    fn review_task_text_mentions_number_and_title() {
        let text = build_review_task_text(&fixture_pr());
        assert!(text.contains("#12"));
        assert!(text.contains("Add feature X"));
        assert!(text.contains("owner/repo"));
    }
}
