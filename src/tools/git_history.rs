use std::process::Command;

use crate::error::{app_error, AppResult};
use crate::tools::types::{Tool, ToolInput, ToolOutput};

const DEFAULT_LOG_LIMIT: usize = 20;
const DEFAULT_BLAME_LIMIT: usize = 80;
const DEFAULT_MAX_CHARS: usize = 20_000;
const MAX_LIMIT: usize = 500;
const MAX_OUTPUT_CHARS: usize = 100_000;

pub struct GitLogTool;
pub struct GitShowTool;
pub struct GitBlameTool;

impl Tool for GitLogTool {
    fn name(&self) -> &str {
        "git_log"
    }

    fn execute(&self, input: ToolInput) -> AppResult<ToolOutput> {
        let limit = parse_limit(input.get("limit"), DEFAULT_LOG_LIMIT)?;
        let max_chars = parse_max_chars(input.get("max_chars"))?;
        let cwd = input.get("cwd").unwrap_or(".");
        let mut args = vec![
            "log".to_string(),
            format!("--max-count={limit}"),
            "--date=short".to_string(),
            "--pretty=format:%h %ad %an %s".to_string(),
        ];
        if let Some(rev) = input.get("ref").filter(|value| !value.trim().is_empty()) {
            validate_ref(rev)?;
            args.push(rev.to_string());
        }
        args.push("--".to_string());
        if let Some(path) = input.get("path").filter(|value| !value.trim().is_empty()) {
            validate_path_arg(path)?;
            args.push(path.to_string());
        }

        run_git(cwd, "log", &args, max_chars)
    }
}

impl Tool for GitShowTool {
    fn name(&self) -> &str {
        "git_show"
    }

    fn execute(&self, input: ToolInput) -> AppResult<ToolOutput> {
        let rev = input
            .get("ref")
            .filter(|value| !value.trim().is_empty())
            .unwrap_or("HEAD");
        validate_ref(rev)?;
        let max_chars = parse_max_chars(input.get("max_chars"))?;
        let cwd = input.get("cwd").unwrap_or(".");
        let mut args = vec![
            "show".to_string(),
            "--stat".to_string(),
            "--patch".to_string(),
            "--find-renames".to_string(),
            "--find-copies".to_string(),
            "--format=fuller".to_string(),
            rev.to_string(),
            "--".to_string(),
        ];
        if let Some(path) = input.get("path").filter(|value| !value.trim().is_empty()) {
            validate_path_arg(path)?;
            args.push(path.to_string());
        }

        run_git(cwd, "show", &args, max_chars)
    }
}

impl Tool for GitBlameTool {
    fn name(&self) -> &str {
        "git_blame"
    }

    fn execute(&self, input: ToolInput) -> AppResult<ToolOutput> {
        let path = input
            .get("path")
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| app_error("git_blame requires a path"))?;
        validate_path_arg(path)?;
        let start = parse_line(input.get("line_start"), 1)?;
        let end = match input.get("line_end") {
            Some(value) if !value.trim().is_empty() => parse_line(Some(value), start)?,
            _ => {
                let limit = parse_limit(input.get("limit"), DEFAULT_BLAME_LIMIT)?;
                start.saturating_add(limit).saturating_sub(1)
            }
        };
        if end < start {
            return Err(app_error("git_blame line_end must be >= line_start"));
        }
        let rev = input
            .get("ref")
            .filter(|value| !value.trim().is_empty())
            .unwrap_or("HEAD");
        validate_ref(rev)?;
        let max_chars = parse_max_chars(input.get("max_chars"))?;
        let cwd = input.get("cwd").unwrap_or(".");
        let args = vec![
            "blame".to_string(),
            "--date=short".to_string(),
            "-L".to_string(),
            format!("{start},{end}"),
            rev.to_string(),
            "--".to_string(),
            path.to_string(),
        ];

        run_git(cwd, "blame", &args, max_chars)
    }
}

fn run_git(cwd: &str, kind: &str, args: &[String], max_chars: usize) -> AppResult<ToolOutput> {
    validate_path_arg(cwd)?;
    let output = Command::new("git").current_dir(cwd).args(args).output()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let exit_code = output.status.code().unwrap_or(-1);
    if !output.status.success() {
        return Err(app_error(format!(
            "git_{kind} failed with exit code {exit_code}: {}",
            first_non_empty_line(&stderr)
                .or_else(|| first_non_empty_line(&stdout))
                .unwrap_or("no output")
        )));
    }

    let body = stdout.trim();
    let body = if body.is_empty() {
        "(no output)".to_string()
    } else {
        clip_chars(body, max_chars)
    };
    let truncated = body.chars().count() >= max_chars && stdout.trim().chars().count() > max_chars;

    let mut summary = String::new();
    summary.push_str(&format!("meta.git_command={kind}\n"));
    summary.push_str(&format!("meta.exit_code={exit_code}\n"));
    summary.push_str("meta.result=ok\n");
    if truncated {
        summary.push_str(&format!(
            "meta.truncated=true\nmeta.max_chars={max_chars}\n"
        ));
    }
    summary.push_str(&body);
    if truncated {
        summary.push_str("\n[output truncated]");
    }

    Ok(ToolOutput { summary })
}

fn parse_limit(raw: Option<&str>, default: usize) -> AppResult<usize> {
    let value = match raw {
        Some(value) if !value.trim().is_empty() => value.trim().parse::<usize>().map_err(|_| {
            app_error(format!(
                "limit must be a positive integer, got `{}`",
                value.trim()
            ))
        })?,
        _ => default,
    };
    if !(1..=MAX_LIMIT).contains(&value) {
        return Err(app_error(format!(
            "limit must be between 1 and {MAX_LIMIT}"
        )));
    }
    Ok(value)
}

fn parse_line(raw: Option<&str>, default: usize) -> AppResult<usize> {
    let value = match raw {
        Some(value) if !value.trim().is_empty() => value.trim().parse::<usize>().map_err(|_| {
            app_error(format!(
                "line must be a positive integer, got `{}`",
                value.trim()
            ))
        })?,
        _ => default,
    };
    if value == 0 {
        return Err(app_error("line must be >= 1"));
    }
    Ok(value)
}

fn parse_max_chars(raw: Option<&str>) -> AppResult<usize> {
    let value = match raw {
        Some(value) if !value.trim().is_empty() => value.trim().parse::<usize>().map_err(|_| {
            app_error(format!(
                "max_chars must be a positive integer, got `{}`",
                value.trim()
            ))
        })?,
        _ => DEFAULT_MAX_CHARS,
    };
    if !(1..=MAX_OUTPUT_CHARS).contains(&value) {
        return Err(app_error(format!(
            "max_chars must be between 1 and {MAX_OUTPUT_CHARS}"
        )));
    }
    Ok(value)
}

fn validate_ref(value: &str) -> AppResult<()> {
    if value.starts_with('-') || value.contains('\0') || value.chars().any(char::is_control) {
        return Err(app_error(
            "git ref must not be an option or contain controls",
        ));
    }
    Ok(())
}

fn validate_path_arg(value: &str) -> AppResult<()> {
    if value.contains('\0') || value.chars().any(char::is_control) {
        return Err(app_error("git path must not contain controls"));
    }
    Ok(())
}

fn first_non_empty_line(text: &str) -> Option<&str> {
    text.lines()
        .find(|line| !line.trim().is_empty())
        .map(str::trim)
}

fn clip_chars(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }
    value.chars().take(max_chars).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::Path;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_repo(name: &str) -> std::path::PathBuf {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "deepseek-git-history-{name}-{}-{suffix}",
            std::process::id()
        ))
    }

    fn git(root: &Path, args: &[&str]) {
        let status = Command::new("git")
            .current_dir(root)
            .args(args)
            .status()
            .unwrap();
        assert!(status.success(), "git {:?} failed", args);
    }

    fn repo_with_commit() -> std::path::PathBuf {
        let root = temp_repo("repo");
        fs::create_dir_all(&root).unwrap();
        git(&root, &["init", "-q"]);
        fs::write(root.join("src.txt"), "alpha\nbeta\ngamma\n").unwrap();
        git(&root, &["add", "src.txt"]);
        git(
            &root,
            &[
                "-c",
                "user.name=Deepseek Test",
                "-c",
                "user.email=test@example.com",
                "commit",
                "-m",
                "initial commit",
            ],
        );
        root
    }

    #[test]
    fn git_log_reads_recent_commits() {
        let root = repo_with_commit();
        let output = GitLogTool
            .execute(
                ToolInput::new()
                    .with_arg("cwd", root.to_string_lossy())
                    .with_arg("limit", "1"),
            )
            .unwrap();

        let _ = fs::remove_dir_all(root);
        assert!(output.summary.contains("meta.git_command=log"));
        assert!(output.summary.contains("meta.result=ok"));
        assert!(output.summary.contains("initial commit"));
    }

    #[test]
    fn git_show_reads_head_patch() {
        let root = repo_with_commit();
        let output = GitShowTool
            .execute(
                ToolInput::new()
                    .with_arg("cwd", root.to_string_lossy())
                    .with_arg("ref", "HEAD"),
            )
            .unwrap();

        let _ = fs::remove_dir_all(root);
        assert!(output.summary.contains("meta.git_command=show"));
        assert!(output.summary.contains("initial commit"));
        assert!(output.summary.contains("+alpha"));
    }

    #[test]
    fn git_blame_reads_line_range() {
        let root = repo_with_commit();
        let output = GitBlameTool
            .execute(
                ToolInput::new()
                    .with_arg("cwd", root.to_string_lossy())
                    .with_arg("path", "src.txt")
                    .with_arg("line_start", "2")
                    .with_arg("line_end", "2"),
            )
            .unwrap();

        let _ = fs::remove_dir_all(root);
        assert!(output.summary.contains("meta.git_command=blame"));
        assert!(output.summary.contains("beta"));
        assert!(!output.summary.contains("alpha"));
    }

    #[test]
    fn git_ref_rejects_option_injection() {
        let error = GitShowTool
            .execute(ToolInput::new().with_arg("ref", "--help"))
            .unwrap_err();
        assert!(error.to_string().contains("must not be an option"));
    }
}
