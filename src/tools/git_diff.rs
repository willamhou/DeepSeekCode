use std::process::Command;

use crate::error::AppResult;
use crate::tools::types::{Tool, ToolInput, ToolOutput};

pub struct GitStatusTool;
pub struct GitDiffTool;

impl Tool for GitStatusTool {
    fn name(&self) -> &str {
        "git_status"
    }

    fn execute(&self, input: ToolInput) -> AppResult<ToolOutput> {
        let cwd = input.get("cwd").unwrap_or(".");
        let mut args = vec![
            "status".to_string(),
            "--porcelain=v1".to_string(),
            "-b".to_string(),
        ];
        if let Some(path) = input.get("path").filter(|value| !value.trim().is_empty()) {
            args.push("--".to_string());
            args.push(path.to_string());
        }

        let output = Command::new("git").args(&args).current_dir(cwd).output()?;
        let summary = if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            if stdout.trim().is_empty() {
                "No git status output.".to_string()
            } else {
                cap_output(&stdout, 40_000)
            }
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            format!("git status failed: {}", stderr.trim())
        };

        Ok(ToolOutput { summary })
    }
}

impl Tool for GitDiffTool {
    fn name(&self) -> &str {
        "git_diff"
    }

    fn execute(&self, input: ToolInput) -> AppResult<ToolOutput> {
        let cwd = input.get("cwd").unwrap_or(".");
        let cached = input
            .get("cached")
            .is_some_and(|value| matches!(value, "1" | "true" | "TRUE" | "yes" | "on"));
        let unified = input
            .get("unified")
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(3)
            .min(50);
        let max_chars = input
            .get("max_chars")
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(40_000)
            .clamp(1_000, 200_000);
        let mut args = vec![
            "diff".to_string(),
            "--no-color".to_string(),
            "--no-ext-diff".to_string(),
            format!("--unified={unified}"),
        ];
        if cached {
            args.push("--cached".to_string());
        }
        if let Some(path) = input.get("path").filter(|value| !value.trim().is_empty()) {
            args.push("--".to_string());
            args.push(path.to_string());
        }

        let output = Command::new("git").args(&args).current_dir(cwd).output()?;
        let summary = if output.status.success() {
            if output.stdout.is_empty() {
                "No local diff.".to_string()
            } else {
                cap_output(&String::from_utf8_lossy(&output.stdout), max_chars)
            }
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            format!("git diff failed: {}", stderr.trim())
        };

        Ok(ToolOutput { summary })
    }
}

fn cap_output(value: &str, max_chars: usize) -> String {
    if value.len() <= max_chars {
        return value.to_string();
    }
    let mut out = value
        .chars()
        .take(max_chars)
        .collect::<String>()
        .trim_end()
        .to_string();
    out.push_str("\n[truncated]");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root(name: &str) -> std::path::PathBuf {
        let suffix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "deepseek-git-diff-{name}-{}-{suffix}",
            std::process::id()
        ))
    }

    #[test]
    fn git_status_reports_branch_and_untracked_files() {
        let root = temp_root("status");
        std::fs::create_dir_all(&root).unwrap();
        let status = Command::new("git")
            .arg("init")
            .current_dir(&root)
            .output()
            .unwrap();
        assert!(status.status.success());
        std::fs::write(root.join("new.txt"), "hello\n").unwrap();

        let output = GitStatusTool
            .execute(ToolInput::new().with_arg("cwd", root.display().to_string()))
            .unwrap();

        assert!(output.summary.contains("##"));
        assert!(output.summary.contains("?? new.txt"));
    }

    #[test]
    fn git_diff_supports_cached_path_and_unified_context() {
        let root = temp_root("diff");
        std::fs::create_dir_all(&root).unwrap();
        let status = Command::new("git")
            .arg("init")
            .current_dir(&root)
            .output()
            .unwrap();
        assert!(status.status.success());
        std::fs::write(root.join("a.txt"), "hello\n").unwrap();
        let status = Command::new("git")
            .args(["add", "a.txt"])
            .current_dir(&root)
            .output()
            .unwrap();
        assert!(status.status.success());

        let output = GitDiffTool
            .execute(
                ToolInput::new()
                    .with_arg("cwd", root.display().to_string())
                    .with_arg("cached", "true")
                    .with_arg("path", "a.txt")
                    .with_arg("unified", "0"),
            )
            .unwrap();

        assert!(output.summary.contains("diff --git"));
        assert!(output.summary.contains("+hello"));
    }
}
