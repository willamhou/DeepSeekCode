use crate::error::AppResult;
use crate::tools::types::{Tool, ToolInput, ToolOutput};
use std::fs;
use std::path::{Path, PathBuf};

pub struct ListFilesTool;
pub struct ListDirTool;

impl Tool for ListFilesTool {
    fn name(&self) -> &str {
        "list_files"
    }

    fn execute(&self, input: ToolInput) -> AppResult<ToolOutput> {
        let root = input.get("root").unwrap_or(".");
        let max_depth = input
            .get("max_depth")
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(3);
        let limit = input
            .get("limit")
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(40);

        let mut files = Vec::new();
        visit(
            Path::new(root),
            Path::new(root),
            0,
            max_depth,
            limit,
            &mut files,
        )?;

        Ok(ToolOutput {
            summary: if files.is_empty() {
                "No files found.".to_string()
            } else {
                files.join("\n")
            },
        })
    }
}

impl Tool for ListDirTool {
    fn name(&self) -> &str {
        "list_dir"
    }

    fn execute(&self, mut input: ToolInput) -> AppResult<ToolOutput> {
        if input.get("root").is_none() {
            if let Some(path) = input.get("path").map(str::to_string) {
                input.args.insert("root".to_string(), path);
            }
        }
        ListFilesTool.execute(input)
    }
}

fn visit(
    root: &Path,
    current: &Path,
    depth: usize,
    max_depth: usize,
    limit: usize,
    files: &mut Vec<String>,
) -> AppResult<()> {
    if files.len() >= limit || depth > max_depth {
        return Ok(());
    }

    let mut entries = fs::read_dir(current)?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .collect::<Vec<PathBuf>>();
    entries.sort();

    for path in entries {
        if files.len() >= limit {
            break;
        }

        let name = path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("");
        if should_skip(name) {
            continue;
        }

        let display = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .display()
            .to_string();

        if path.is_dir() {
            files.push(format!("{display}/"));
            visit(root, &path, depth + 1, max_depth, limit, files)?;
        } else {
            files.push(display);
        }
    }

    Ok(())
}

fn should_skip(name: &str) -> bool {
    matches!(
        name,
        ".git" | "target" | "node_modules" | "dist" | ".dscode" | "__pycache__"
    )
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
            "deepseek-list-files-{name}-{}-{suffix}",
            std::process::id()
        ))
    }

    #[test]
    fn list_dir_maps_path_to_list_files_root() {
        let root = temp_root("alias");
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/main.rs"), "fn main() {}\n").unwrap();

        let output = ListDirTool
            .execute(
                ToolInput::new()
                    .with_arg("path", root.display().to_string())
                    .with_arg("max_depth", "1")
                    .with_arg("limit", "10"),
            )
            .unwrap();

        assert!(output.summary.contains("src/"));
        assert!(output.summary.contains("src/main.rs"));
    }
}
