use std::process::Command;

use crate::error::{app_error, AppResult};
use crate::tools::types::{Tool, ToolInput, ToolOutput};
use crate::util::json::{
    json_as_array, json_as_object, json_as_string, json_as_u64, parse_json_value,
    parse_root_object, JsonValue,
};

const DEFAULT_MAX_CHARS: usize = 20_000;
const HARD_MAX_CHARS: usize = 100_000;
const DEFAULT_DIFF_CHARS: usize = 20_000;
const HARD_DIFF_CHARS: usize = 100_000;

pub struct GithubPrContextTool;
pub struct GithubIssueContextTool;
pub struct GithubCommentTool;
pub struct GithubPrReviewCommentTool;
pub struct GithubCloseIssueTool;

impl Tool for GithubPrContextTool {
    fn name(&self) -> &str {
        "github_pr_context"
    }

    fn execute(&self, input: ToolInput) -> AppResult<ToolOutput> {
        let number = required_reference(&input, "github_pr_context")?;
        let max_chars =
            parse_usize_arg(&input, "max_chars", DEFAULT_MAX_CHARS).clamp(1, HARD_MAX_CHARS);
        let mut args = vec![
            "pr".to_string(),
            "view".to_string(),
            number.clone(),
            "--json".to_string(),
            "number,title,state,author,body,comments,reviews,reviewDecision,statusCheckRollup,baseRefName,headRefName,headRefOid,baseRefOid,files,url,createdAt,updatedAt".to_string(),
        ];
        append_repo_args(&mut args, &input);
        let raw = run_gh(&args)?;
        let root = parse_root_object(&raw)?;
        let number = root
            .get("number")
            .and_then(json_as_u64)
            .map(|value| value.to_string())
            .unwrap_or_else(|| number.clone());
        let title = root
            .get("title")
            .and_then(json_as_string)
            .unwrap_or("(untitled)");
        let state = root.get("state").and_then(json_as_string).unwrap_or("-");
        let files = root
            .get("files")
            .and_then(json_as_array)
            .map(|items| items.len())
            .unwrap_or(0);
        let comments = root
            .get("comments")
            .and_then(json_as_array)
            .map(|items| items.len())
            .unwrap_or(0);
        let reviews = root
            .get("reviews")
            .and_then(json_as_array)
            .map(|items| items.len())
            .unwrap_or(0);

        let mut summary = String::new();
        summary.push_str("meta.kind=pr\n");
        summary.push_str(&format!("meta.number={number}\n"));
        summary.push_str(&format!("meta.state={}\n", sanitize_meta(state)));
        summary.push_str(&format!("meta.files={files}\n"));
        summary.push_str(&format!("meta.comments={comments}\n"));
        summary.push_str(&format!("meta.reviews={reviews}\n"));
        summary.push_str(&format!("PR #{number}: {title}\n"));
        summary.push_str("json:\n");
        summary.push_str(&clip(&raw, max_chars));
        summary.push('\n');

        if bool_arg(&input, "include_diff", false) {
            let diff_chars = parse_usize_arg(&input, "diff_max_chars", DEFAULT_DIFF_CHARS)
                .clamp(1, HARD_DIFF_CHARS);
            let mut diff_args = vec![
                "pr".to_string(),
                "diff".to_string(),
                number.clone(),
                "--patch".to_string(),
            ];
            append_repo_args(&mut diff_args, &input);
            let diff = run_gh(&diff_args)?;
            summary.push_str("diff:\n");
            summary.push_str(&clip(&diff, diff_chars));
            summary.push('\n');
        }

        Ok(ToolOutput { summary })
    }
}

impl Tool for GithubIssueContextTool {
    fn name(&self) -> &str {
        "github_issue_context"
    }

    fn execute(&self, input: ToolInput) -> AppResult<ToolOutput> {
        let number = required_reference(&input, "github_issue_context")?;
        let include_comments = bool_arg(&input, "include_comments", true);
        let fields = if include_comments {
            "number,title,state,author,labels,assignees,milestone,body,comments,url,createdAt,updatedAt"
        } else {
            "number,title,state,author,labels,assignees,milestone,body,url,createdAt,updatedAt"
        };
        let max_chars =
            parse_usize_arg(&input, "max_chars", DEFAULT_MAX_CHARS).clamp(1, HARD_MAX_CHARS);
        let mut args = vec![
            "issue".to_string(),
            "view".to_string(),
            number.clone(),
            "--json".to_string(),
            fields.to_string(),
        ];
        append_repo_args(&mut args, &input);
        let raw = run_gh(&args)?;
        let root = parse_root_object(&raw)?;
        let number = root
            .get("number")
            .and_then(json_as_u64)
            .map(|value| value.to_string())
            .unwrap_or_else(|| number.clone());
        let title = root
            .get("title")
            .and_then(json_as_string)
            .unwrap_or("(untitled)");
        let state = root.get("state").and_then(json_as_string).unwrap_or("-");
        let labels = root
            .get("labels")
            .and_then(json_as_array)
            .map(|items| items.len())
            .unwrap_or(0);
        let comments = root
            .get("comments")
            .and_then(json_as_array)
            .map(|items| items.len())
            .unwrap_or(0);

        let mut summary = String::new();
        summary.push_str("meta.kind=issue\n");
        summary.push_str(&format!("meta.number={number}\n"));
        summary.push_str(&format!("meta.state={}\n", sanitize_meta(state)));
        summary.push_str(&format!("meta.labels={labels}\n"));
        summary.push_str(&format!("meta.comments={comments}\n"));
        summary.push_str(&format!("Issue #{number}: {title}\n"));
        summary.push_str("json:\n");
        summary.push_str(&clip(&raw, max_chars));
        summary.push('\n');
        Ok(ToolOutput { summary })
    }
}

impl Tool for GithubCommentTool {
    fn name(&self) -> &str {
        "github_comment"
    }

    fn execute(&self, input: ToolInput) -> AppResult<ToolOutput> {
        let target = required_arg(&input, "target", "github_comment")?;
        if target != "issue" && target != "pr" {
            return Err(app_error("github_comment target must be `issue` or `pr`"));
        }
        let number = required_reference(&input, "github_comment")?;
        validate_positive_number(&number, "number")?;
        let body = required_arg(&input, "body", "github_comment")?;
        validate_nonempty_object_arg(&input, "evidence", "github_comment")?;
        if bool_arg(&input, "dry_run", false) {
            return Ok(ToolOutput {
                summary: format!("Dry run: would comment on {target} #{number}."),
            });
        }

        let subcmd = if target == "pr" { "pr" } else { "issue" };
        let mut args = vec![
            subcmd.to_string(),
            "comment".to_string(),
            number.clone(),
            "--body".to_string(),
            body,
        ];
        append_repo_args(&mut args, &input);
        run_gh(&args)?;

        Ok(ToolOutput {
            summary: format!("meta.kind=github_comment\nmeta.target={target}\nmeta.number={number}\nCommented on {target} #{number}."),
        })
    }
}

impl Tool for GithubPrReviewCommentTool {
    fn name(&self) -> &str {
        "github_pr_review_comment"
    }

    fn execute(&self, input: ToolInput) -> AppResult<ToolOutput> {
        let number = required_reference(&input, "github_pr_review_comment")?;
        validate_positive_number(&number, "number")?;
        validate_nonempty_object_arg(&input, "evidence", "github_pr_review_comment")?;
        let comments = pr_review_comment_specs_from_input(&input)?;
        if comments.is_empty() {
            return Err(app_error(
                "github_pr_review_comment requires at least one inline comment",
            ));
        }
        if bool_arg(&input, "dry_run", false) {
            return Ok(ToolOutput {
                summary: format!(
                    "Dry run: would post {} inline review comment(s) on PR #{}.",
                    comments.len(),
                    number
                ),
            });
        }

        let endpoint = github_pr_review_comments_endpoint(&input, &number);
        for comment in &comments {
            let mut args = vec![
                "api".to_string(),
                "--method".to_string(),
                "POST".to_string(),
                endpoint.clone(),
                "-f".to_string(),
                format!("body={}", comment.body),
                "-f".to_string(),
                format!("commit_id={}", comment.commit_id),
                "-f".to_string(),
                format!("path={}", comment.path),
                "-F".to_string(),
                format!("line={}", comment.line),
                "-f".to_string(),
                format!("side={}", comment.side),
            ];
            if let Some(start_line) = comment.start_line {
                args.push("-F".to_string());
                args.push(format!("start_line={start_line}"));
            }
            if let Some(start_side) = comment.start_side.as_ref() {
                args.push("-f".to_string());
                args.push(format!("start_side={start_side}"));
            }
            run_gh(&args)?;
        }

        Ok(ToolOutput {
            summary: format!(
                "meta.kind=github_pr_review_comment\nmeta.number={number}\nmeta.comment_count={}\nPosted {} inline review comment(s) on PR #{number}.",
                comments.len(),
                comments.len()
            ),
        })
    }
}

impl Tool for GithubCloseIssueTool {
    fn name(&self) -> &str {
        "github_close_issue"
    }

    fn execute(&self, input: ToolInput) -> AppResult<ToolOutput> {
        let number = required_reference(&input, "github_close_issue")?;
        validate_positive_number(&number, "number")?;
        validate_nonempty_array_arg(&input, "acceptance_criteria", "github_close_issue")?;
        validate_close_issue_evidence(&input)?;
        if !bool_arg(&input, "allow_dirty", false) {
            let status = git_status_porcelain(&input)?;
            if !status.trim().is_empty() {
                return Err(app_error(
                    "refusing to close issue: worktree is dirty and allow_dirty was false",
                ));
            }
        }
        if bool_arg(&input, "dry_run", false) {
            return Ok(ToolOutput {
                summary: format!("Dry run: would close issue #{number}."),
            });
        }

        if let Some(comment) = input
            .get("comment")
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            let mut comment_args = vec![
                "issue".to_string(),
                "comment".to_string(),
                number.clone(),
                "--body".to_string(),
                comment.to_string(),
            ];
            append_repo_args(&mut comment_args, &input);
            run_gh(&comment_args)?;
        }

        let mut args = vec![
            "issue".to_string(),
            "close".to_string(),
            number.clone(),
            "--reason".to_string(),
            "completed".to_string(),
        ];
        append_repo_args(&mut args, &input);
        run_gh(&args)?;

        Ok(ToolOutput {
            summary: format!(
                "meta.kind=github_close_issue\nmeta.number={number}\nClosed issue #{number}."
            ),
        })
    }
}

fn required_arg(input: &ToolInput, key: &str, tool_name: &str) -> AppResult<String> {
    input
        .get(key)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .ok_or_else(|| app_error(format!("{tool_name} requires `{key}`")))
}

fn required_reference(input: &ToolInput, tool_name: &str) -> AppResult<String> {
    input
        .get("number")
        .or_else(|| input.get("pr"))
        .or_else(|| input.get("issue"))
        .or_else(|| input.get("ref"))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .ok_or_else(|| app_error(format!("{tool_name} requires `number`")))
}

fn validate_positive_number(value: &str, key: &str) -> AppResult<()> {
    let parsed = value
        .trim()
        .parse::<u64>()
        .map_err(|_| app_error(format!("{key} must be a positive integer")))?;
    if parsed == 0 {
        return Err(app_error(format!("{key} must be a positive integer")));
    }
    Ok(())
}

fn validate_nonempty_object_arg(input: &ToolInput, key: &str, tool_name: &str) -> AppResult<()> {
    let raw = required_arg(input, key, tool_name)?;
    let object = parse_root_object(&raw)
        .map_err(|error| app_error(format!("{tool_name} requires `{key}` JSON object: {error}")))?;
    if object.is_empty() {
        return Err(app_error(format!(
            "{tool_name} requires `{key}` to include evidence fields"
        )));
    }
    Ok(())
}

fn validate_nonempty_array_arg(input: &ToolInput, key: &str, tool_name: &str) -> AppResult<()> {
    let raw = required_arg(input, key, tool_name)?;
    let value = parse_json_value(raw.trim())
        .map_err(|error| app_error(format!("{tool_name} requires `{key}` JSON array: {error}")))?;
    let JsonValue::Array(items) = value else {
        return Err(app_error(format!(
            "{tool_name} requires `{key}` JSON array"
        )));
    };
    if items.is_empty() {
        return Err(app_error(format!(
            "{tool_name} requires `{key}` to contain at least one item"
        )));
    }
    Ok(())
}

fn validate_close_issue_evidence(input: &ToolInput) -> AppResult<()> {
    let raw = required_arg(input, "evidence", "github_close_issue")?;
    let root = parse_root_object(&raw).map_err(|error| {
        app_error(format!(
            "github_close_issue requires `evidence` JSON object: {error}"
        ))
    })?;
    require_nonempty_json_array_field(&root, "files_changed")?;
    require_nonempty_json_array_field(&root, "tests_run")?;
    let final_status = root
        .get("final_status")
        .and_then(json_as_string)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| app_error("github_close_issue evidence requires `final_status`"))?;
    if !final_status.eq_ignore_ascii_case("completed")
        && !final_status.eq_ignore_ascii_case("done")
        && !final_status.eq_ignore_ascii_case("fixed")
    {
        return Err(app_error(
            "github_close_issue evidence final_status must be completed, done, or fixed",
        ));
    }
    Ok(())
}

#[derive(Debug, Clone)]
struct PrReviewCommentSpec {
    path: String,
    line: u64,
    body: String,
    commit_id: String,
    side: String,
    start_line: Option<u64>,
    start_side: Option<String>,
}

fn pr_review_comment_specs_from_input(input: &ToolInput) -> AppResult<Vec<PrReviewCommentSpec>> {
    let default_commit_id = optional_commit_id_arg(input);
    if let Some(raw) = input
        .get("comments")
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        let value = parse_json_value(raw).map_err(|error| {
            app_error(format!(
                "github_pr_review_comment requires `comments` JSON array: {error}"
            ))
        })?;
        let JsonValue::Array(items) = value else {
            return Err(app_error(
                "github_pr_review_comment requires `comments` JSON array",
            ));
        };
        let mut comments = Vec::new();
        for (index, item) in items.iter().enumerate() {
            let object = json_as_object(item).ok_or_else(|| {
                app_error(format!(
                    "github_pr_review_comment comments[{index}] must be an object"
                ))
            })?;
            comments.push(pr_review_comment_spec_from_object(
                object,
                default_commit_id.as_deref(),
                index,
            )?);
        }
        return Ok(comments);
    }

    Ok(vec![PrReviewCommentSpec {
        path: required_arg(input, "path", "github_pr_review_comment")?,
        line: parse_positive_u64_arg(
            required_arg(input, "line", "github_pr_review_comment")?.as_str(),
            "line",
        )?,
        body: required_arg(input, "body", "github_pr_review_comment")?,
        commit_id: default_commit_id
            .ok_or_else(|| app_error("github_pr_review_comment requires `commit_id`"))?,
        side: review_comment_side(input.get("side"), "side")?,
        start_line: optional_positive_u64_arg(input.get("start_line"), "start_line")?,
        start_side: optional_review_comment_side(input.get("start_side"), "start_side")?,
    }])
}

fn pr_review_comment_spec_from_object(
    object: &std::collections::BTreeMap<String, JsonValue>,
    default_commit_id: Option<&str>,
    index: usize,
) -> AppResult<PrReviewCommentSpec> {
    let label = |key: &str| format!("comments[{index}].{key}");
    let path = json_object_string_arg(object, "path").ok_or_else(|| {
        app_error(format!(
            "github_pr_review_comment requires `{}`",
            label("path")
        ))
    })?;
    let line = json_object_u64_arg(object, "line")
        .ok_or_else(|| {
            app_error(format!(
                "github_pr_review_comment requires `{}`",
                label("line")
            ))
        })
        .and_then(|value| validate_positive_u64(value, &label("line")))?;
    let body = json_object_string_arg(object, "body").ok_or_else(|| {
        app_error(format!(
            "github_pr_review_comment requires `{}`",
            label("body")
        ))
    })?;
    let commit_id = json_object_string_arg(object, "commit_id")
        .or_else(|| json_object_string_arg(object, "head_sha"))
        .or_else(|| json_object_string_arg(object, "sha"))
        .or_else(|| default_commit_id.map(str::to_string))
        .ok_or_else(|| {
            app_error(format!(
                "github_pr_review_comment requires `{}` or top-level `commit_id`",
                label("commit_id")
            ))
        })?;
    Ok(PrReviewCommentSpec {
        path,
        line,
        body,
        commit_id,
        side: review_comment_side(json_object_string_ref(object, "side"), &label("side"))?,
        start_line: json_object_u64_arg(object, "start_line")
            .map(|value| validate_positive_u64(value, &label("start_line")))
            .transpose()?,
        start_side: optional_review_comment_side(
            json_object_string_ref(object, "start_side"),
            &label("start_side"),
        )?,
    })
}

fn optional_commit_id_arg(input: &ToolInput) -> Option<String> {
    input
        .get("commit_id")
        .or_else(|| input.get("head_sha"))
        .or_else(|| input.get("sha"))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn parse_positive_u64_arg(value: &str, key: &str) -> AppResult<u64> {
    let parsed = value
        .trim()
        .parse::<u64>()
        .map_err(|_| app_error(format!("{key} must be a positive integer")))?;
    validate_positive_u64(parsed, key)
}

fn validate_positive_u64(value: u64, key: &str) -> AppResult<u64> {
    if value == 0 {
        return Err(app_error(format!("{key} must be a positive integer")));
    }
    Ok(value)
}

fn optional_positive_u64_arg(value: Option<&str>, key: &str) -> AppResult<Option<u64>> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| parse_positive_u64_arg(value, key))
        .transpose()
}

fn review_comment_side(value: Option<&str>, key: &str) -> AppResult<String> {
    let side = value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("RIGHT")
        .to_ascii_uppercase();
    if side != "RIGHT" && side != "LEFT" {
        return Err(app_error(format!("{key} must be RIGHT or LEFT")));
    }
    Ok(side)
}

fn optional_review_comment_side(value: Option<&str>, key: &str) -> AppResult<Option<String>> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| review_comment_side(Some(value), key))
        .transpose()
}

fn json_object_string_arg(
    object: &std::collections::BTreeMap<String, JsonValue>,
    key: &str,
) -> Option<String> {
    json_object_string_ref(object, key).map(str::to_string)
}

fn json_object_string_ref<'a>(
    object: &'a std::collections::BTreeMap<String, JsonValue>,
    key: &str,
) -> Option<&'a str> {
    object
        .get(key)
        .and_then(json_as_string)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn json_object_u64_arg(
    object: &std::collections::BTreeMap<String, JsonValue>,
    key: &str,
) -> Option<u64> {
    object.get(key).and_then(|value| match value {
        JsonValue::Number(text) | JsonValue::String(text) => text.trim().parse::<u64>().ok(),
        _ => None,
    })
}

fn github_pr_review_comments_endpoint(input: &ToolInput, number: &str) -> String {
    if let Some(repo) = input
        .get("repo")
        .or_else(|| input.get("repository"))
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return format!("repos/{repo}/pulls/{number}/comments");
    }
    format!("repos/{{owner}}/{{repo}}/pulls/{number}/comments")
}

fn require_nonempty_json_array_field(
    root: &std::collections::BTreeMap<String, JsonValue>,
    key: &str,
) -> AppResult<()> {
    let items = root.get(key).and_then(json_as_array).ok_or_else(|| {
        app_error(format!(
            "github_close_issue evidence requires `{key}` array"
        ))
    })?;
    if items.is_empty() {
        return Err(app_error(format!(
            "github_close_issue evidence `{key}` must not be empty"
        )));
    }
    Ok(())
}

fn append_repo_args(args: &mut Vec<String>, input: &ToolInput) {
    if let Some(repo) = input
        .get("repo")
        .or_else(|| input.get("repository"))
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        args.push("-R".to_string());
        args.push(repo.to_string());
    }
}

fn run_gh(args: &[String]) -> AppResult<String> {
    let gh = std::env::var("DSCODE_GH_BIN").unwrap_or_else(|_| "gh".to_string());
    let output = Command::new(&gh).args(args).output().map_err(|error| {
        app_error(format!(
            "failed to run gh executable `{}`: {error}",
            sanitize_meta(&gh)
        ))
    })?;
    if !output.status.success() {
        return Err(app_error(format!(
            "gh {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

fn git_status_porcelain(input: &ToolInput) -> AppResult<String> {
    let mut command = Command::new("git");
    if let Some(cwd) = input
        .get("cwd")
        .or_else(|| input.get("workdir"))
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        command.args(["-C", cwd]);
    }
    let output = command
        .args(["status", "--porcelain"])
        .output()
        .map_err(|error| app_error(format!("failed to run git status: {error}")))?;
    if !output.status.success() {
        return Err(app_error(format!(
            "git status failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

fn bool_arg(input: &ToolInput, key: &str, default: bool) -> bool {
    input
        .get(key)
        .map(|value| matches!(value.trim(), "1" | "true" | "TRUE" | "yes" | "on"))
        .unwrap_or(default)
}

fn parse_usize_arg(input: &ToolInput, key: &str, default: usize) -> usize {
    input
        .get(key)
        .and_then(|value| value.trim().parse::<usize>().ok())
        .unwrap_or(default)
}

fn clip(value: &str, max_chars: usize) -> String {
    let mut out = String::new();
    let mut count = 0usize;
    for ch in value.chars() {
        if count >= max_chars {
            out.push_str("\n[truncated]\n");
            break;
        }
        out.push(ch);
        count += 1;
    }
    out
}

fn sanitize_meta(value: &str) -> String {
    value.replace(['\n', '\r'], " ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::{Mutex, OnceLock};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn env_lock() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(())).lock().unwrap()
    }

    fn temp_root(label: &str) -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "deepseek-github-tool-{label}-{}-{nanos}",
            std::process::id()
        ));
        fs::create_dir_all(&root).unwrap();
        root
    }

    #[cfg(unix)]
    fn fake_gh() -> std::path::PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let path = temp_root("fake-gh").join("gh");
        fs::write(
            &path,
            r#"#!/bin/sh
if [ "$1" = "pr" ] && [ "$2" = "view" ]; then
  printf '%s\n' '{"number":7,"title":"Fix parser","state":"OPEN","author":{"login":"octo"},"body":"body","comments":[{"body":"comment"}],"reviews":[{"state":"APPROVED"}],"reviewDecision":"APPROVED","statusCheckRollup":[],"baseRefName":"main","headRefName":"fix-parser","headRefOid":"abc","baseRefOid":"def","files":[{"path":"src/lib.rs"}],"url":"https://github.com/o/r/pull/7","createdAt":"2026-05-13T00:00:00Z","updatedAt":"2026-05-13T00:00:00Z"}'
  exit 0
fi
if [ "$1" = "pr" ] && [ "$2" = "diff" ]; then
  printf '%s\n' 'diff --git a/src/lib.rs b/src/lib.rs'
  exit 0
fi
if [ "$1" = "pr" ] && [ "$2" = "comment" ]; then
  printf '%s\n' 'commented pr'
  exit 0
fi
if [ "$1" = "api" ] && [ "$2" = "--method" ] && [ "$3" = "POST" ]; then
  case "$4" in
    repos/*/pulls/7/comments)
      printf '%s\n' '{"id":123,"path":"src/lib.rs","line":12}'
      exit 0
      ;;
  esac
fi
if [ "$1" = "issue" ] && [ "$2" = "view" ]; then
  printf '%s\n' '{"number":9,"title":"Bug report","state":"OPEN","author":{"login":"octo"},"labels":[{"name":"bug"}],"assignees":[],"milestone":null,"body":"issue body","comments":[{"body":"comment"}],"url":"https://github.com/o/r/issues/9","createdAt":"2026-05-13T00:00:00Z","updatedAt":"2026-05-13T00:00:00Z"}'
  exit 0
fi
if [ "$1" = "issue" ] && [ "$2" = "comment" ]; then
  printf '%s\n' 'commented issue'
  exit 0
fi
if [ "$1" = "issue" ] && [ "$2" = "close" ]; then
  printf '%s\n' 'closed issue'
  exit 0
fi
echo "unexpected gh args: $*" >&2
exit 2
"#,
        )
        .unwrap();
        let mut perms = fs::metadata(&path).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&path, perms).unwrap();
        path
    }

    #[cfg(unix)]
    #[test]
    fn github_pr_context_reads_pr_and_diff_from_gh() {
        let _guard = env_lock();
        let gh = fake_gh();
        std::env::set_var("DSCODE_GH_BIN", &gh);
        let output = GithubPrContextTool
            .execute(
                ToolInput::new()
                    .with_arg("number", "7")
                    .with_arg("include_diff", "true"),
            )
            .unwrap();
        std::env::remove_var("DSCODE_GH_BIN");

        assert!(output.summary.contains("meta.kind=pr"));
        assert!(output.summary.contains("meta.number=7"));
        assert!(output.summary.contains("PR #7: Fix parser"));
        assert!(output.summary.contains("diff --git"));
    }

    #[cfg(unix)]
    #[test]
    fn github_issue_context_reads_issue_from_gh() {
        let _guard = env_lock();
        let gh = fake_gh();
        std::env::set_var("DSCODE_GH_BIN", &gh);
        let output = GithubIssueContextTool
            .execute(ToolInput::new().with_arg("number", "9"))
            .unwrap();
        std::env::remove_var("DSCODE_GH_BIN");

        assert!(output.summary.contains("meta.kind=issue"));
        assert!(output.summary.contains("meta.labels=1"));
        assert!(output.summary.contains("Issue #9: Bug report"));
    }

    #[test]
    fn github_comment_dry_run_validates_evidence_without_gh() {
        let output = GithubCommentTool
            .execute(
                ToolInput::new()
                    .with_arg("target", "issue")
                    .with_arg("number", "9")
                    .with_arg("body", "blocked on CI")
                    .with_arg("evidence", r#"{"tests_run":["cargo test"]}"#)
                    .with_arg("dry_run", "true"),
            )
            .unwrap();

        assert!(output
            .summary
            .contains("Dry run: would comment on issue #9"));
    }

    #[cfg(unix)]
    #[test]
    fn github_comment_posts_pr_comment_with_fake_gh() {
        let _guard = env_lock();
        let gh = fake_gh();
        std::env::set_var("DSCODE_GH_BIN", &gh);
        let output = GithubCommentTool
            .execute(
                ToolInput::new()
                    .with_arg("target", "pr")
                    .with_arg("number", "7")
                    .with_arg("body", "verified")
                    .with_arg("evidence", r#"{"tests_run":["cargo test"]}"#),
            )
            .unwrap();
        std::env::remove_var("DSCODE_GH_BIN");

        assert!(output.summary.contains("meta.kind=github_comment"));
        assert!(output.summary.contains("meta.target=pr"));
        assert!(output.summary.contains("Commented on pr #7"));
    }

    #[test]
    fn github_pr_review_comment_dry_run_validates_batch_without_gh() {
        let output = GithubPrReviewCommentTool
            .execute(
                ToolInput::new()
                    .with_arg("number", "7")
                    .with_arg("commit_id", "abc123")
                    .with_arg(
                        "comments",
                        r#"[{"path":"src/lib.rs","line":12,"body":"check this"}]"#,
                    )
                    .with_arg("evidence", r#"{"tool":"review","issue_count":1}"#)
                    .with_arg("dry_run", "true"),
            )
            .unwrap();

        assert!(output
            .summary
            .contains("would post 1 inline review comment(s) on PR #7"));
    }

    #[cfg(unix)]
    #[test]
    fn github_pr_review_comment_posts_inline_comment_with_fake_gh() {
        let _guard = env_lock();
        let gh = fake_gh();
        std::env::set_var("DSCODE_GH_BIN", &gh);
        let output = GithubPrReviewCommentTool
            .execute(
                ToolInput::new()
                    .with_arg("number", "7")
                    .with_arg("repo", "o/r")
                    .with_arg("path", "src/lib.rs")
                    .with_arg("line", "12")
                    .with_arg("body", "check this")
                    .with_arg("commit_id", "abc123")
                    .with_arg("evidence", r#"{"tool":"review","issue_count":1}"#),
            )
            .unwrap();
        std::env::remove_var("DSCODE_GH_BIN");

        assert!(output
            .summary
            .contains("meta.kind=github_pr_review_comment"));
        assert!(output.summary.contains("meta.comment_count=1"));
        assert!(output
            .summary
            .contains("Posted 1 inline review comment(s) on PR #7"));
    }

    #[test]
    fn github_close_issue_refuses_dirty_worktree_by_default() {
        let root = temp_root("dirty-close");
        Command::new("git").arg("init").arg(&root).output().unwrap();
        fs::write(root.join("dirty.txt"), "dirty\n").unwrap();

        let err = GithubCloseIssueTool
            .execute(
                ToolInput::new()
                    .with_arg("number", "9")
                    .with_arg("acceptance_criteria", r#"["bug fixed"]"#)
                    .with_arg(
                        "evidence",
                        r#"{"files_changed":["src/lib.rs"],"tests_run":["cargo test"],"final_status":"completed"}"#,
                    )
                    .with_arg("cwd", root.display().to_string())
                    .with_arg("dry_run", "true"),
            )
            .unwrap_err();

        assert!(err.to_string().contains("worktree is dirty"));
    }

    #[cfg(unix)]
    #[test]
    fn github_close_issue_closes_with_fake_gh_when_allowed_dirty() {
        let _guard = env_lock();
        let gh = fake_gh();
        std::env::set_var("DSCODE_GH_BIN", &gh);
        let output = GithubCloseIssueTool
            .execute(
                ToolInput::new()
                    .with_arg("number", "9")
                    .with_arg("acceptance_criteria", r#"["bug fixed"]"#)
                    .with_arg(
                        "evidence",
                        r#"{"files_changed":["src/lib.rs"],"tests_run":["cargo test"],"final_status":"completed"}"#,
                    )
                    .with_arg("comment", "closing with evidence")
                    .with_arg("allow_dirty", "true"),
            )
            .unwrap();
        std::env::remove_var("DSCODE_GH_BIN");

        assert!(output.summary.contains("meta.kind=github_close_issue"));
        assert!(output.summary.contains("Closed issue #9"));
    }
}
