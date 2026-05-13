use crate::error::AppResult;
use crate::tools::types::{Tool, ToolInput, ToolOutput};
use std::fs;
use std::path::{Path, PathBuf};

const DEFAULT_DEPTH: usize = 3;
const MAX_DEPTH: usize = 8;
const DEFAULT_LIMIT: usize = 120;
const MAX_LIMIT: usize = 500;

pub struct ProjectMapTool;

#[derive(Debug, Default)]
struct ProjectMap {
    tree: Vec<String>,
    key_files: Vec<String>,
    dirs: usize,
    files: usize,
    truncated: bool,
}

impl Tool for ProjectMapTool {
    fn name(&self) -> &str {
        "project_map"
    }

    fn execute(&self, input: ToolInput) -> AppResult<ToolOutput> {
        let root = input
            .get("path")
            .or_else(|| input.get("root"))
            .or_else(|| input.get("cwd"))
            .unwrap_or(".");
        let max_depth = input
            .get("max_depth")
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(DEFAULT_DEPTH)
            .clamp(1, MAX_DEPTH);
        let limit = input
            .get("limit")
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(DEFAULT_LIMIT)
            .clamp(1, MAX_LIMIT);

        let root = Path::new(root);
        let mut map = ProjectMap {
            tree: vec!["./".to_string()],
            ..ProjectMap::default()
        };
        visit_project(root, root, 0, max_depth, limit, &mut map)?;

        let key_files = if map.key_files.is_empty() {
            "(none found)".to_string()
        } else {
            map.key_files.join(", ")
        };
        let truncated = if map.truncated { "\n[truncated]" } else { "" };
        let summary = format!(
            "summary:\nroot: {}\ndirectories: {}\nfiles: {}\nkey_files: {}\n\ntree:\n{}{}",
            root.display(),
            map.dirs,
            map.files,
            key_files,
            map.tree.join("\n"),
            truncated
        );

        Ok(ToolOutput { summary })
    }
}

fn visit_project(
    root: &Path,
    current: &Path,
    depth: usize,
    max_depth: usize,
    limit: usize,
    map: &mut ProjectMap,
) -> AppResult<()> {
    if depth >= max_depth || map.tree.len() >= limit {
        map.truncated |= map.tree.len() >= limit;
        return Ok(());
    }

    let mut entries = fs::read_dir(current)?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .collect::<Vec<PathBuf>>();
    entries.sort();

    for path in entries {
        if map.tree.len() >= limit {
            map.truncated = true;
            break;
        }

        let name = path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("");
        if should_skip(name) {
            continue;
        }

        let Ok(metadata) = fs::symlink_metadata(&path) else {
            continue;
        };
        if metadata.file_type().is_symlink() {
            continue;
        }

        let display = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .display()
            .to_string();
        let indent = "  ".repeat(depth + 1);
        if metadata.is_dir() {
            map.dirs += 1;
            map.tree.push(format!("{indent}{display}/"));
            visit_project(root, &path, depth + 1, max_depth, limit, map)?;
        } else {
            map.files += 1;
            if is_key_file(name) {
                map.key_files.push(display.clone());
            }
            map.tree.push(format!("{indent}{display}"));
        }
    }

    Ok(())
}

fn is_key_file(name: &str) -> bool {
    matches!(
        name.to_lowercase().as_str(),
        "agents.md"
            | "cargo.toml"
            | "go.mod"
            | "package.json"
            | "pnpm-lock.yaml"
            | "pyproject.toml"
            | "readme.md"
            | "requirements.txt"
            | "rust-toolchain.toml"
            | "tsconfig.json"
    )
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
            "deepseek-project-map-{name}-{}-{suffix}",
            std::process::id()
        ))
    }

    #[test]
    fn project_map_renders_tree_summary_and_key_files() {
        let root = temp_root("tree");
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::create_dir_all(root.join("target")).unwrap();
        std::fs::write(root.join("Cargo.toml"), "[package]\nname = \"demo\"\n").unwrap();
        std::fs::write(root.join("src/main.rs"), "fn main() {}\n").unwrap();
        std::fs::write(root.join("target/ignored.rs"), "ignored\n").unwrap();

        let output = ProjectMapTool
            .execute(
                ToolInput::new()
                    .with_arg("path", root.display().to_string())
                    .with_arg("max_depth", "2")
                    .with_arg("limit", "20"),
            )
            .unwrap();

        assert!(output.summary.contains("key_files: Cargo.toml"));
        assert!(output.summary.contains("src/"));
        assert!(output.summary.contains("src/main.rs"));
        assert!(!output.summary.contains("target/ignored.rs"));
    }
}
