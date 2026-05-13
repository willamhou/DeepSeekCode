use crate::error::app_error;
use crate::error::AppResult;
use crate::tools::types::{Tool, ToolInput, ToolOutput};
use std::fs;
use std::path::{Path, PathBuf};

pub struct SearchTextTool;
pub struct GrepFilesTool;

impl Tool for SearchTextTool {
    fn name(&self) -> &str {
        "search_text"
    }

    fn execute(&self, input: ToolInput) -> AppResult<ToolOutput> {
        let query = input
            .get("query")
            .ok_or_else(|| app_error("search_text requires a query"))?;
        let root = input.get("root").unwrap_or(".");
        let limit = input
            .get("limit")
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(20);

        let mut matches = Vec::new();
        search_dir(Path::new(root), Path::new(root), query, limit, &mut matches)?;

        Ok(ToolOutput {
            summary: if matches.is_empty() {
                format!("No matches for `{query}`.")
            } else {
                matches.join("\n")
            },
        })
    }
}

impl Tool for GrepFilesTool {
    fn name(&self) -> &str {
        "grep_files"
    }

    fn execute(&self, mut input: ToolInput) -> AppResult<ToolOutput> {
        let pattern = input
            .get("pattern")
            .or_else(|| input.get("query"))
            .filter(|value| !value.trim().is_empty())
            .map(str::to_string)
            .ok_or_else(|| app_error("grep_files requires a pattern"))?;
        if input.get("query").is_none() {
            input.args.insert("query".to_string(), pattern);
        }
        if input.get("root").is_none() {
            if let Some(path) = input.get("path").map(str::to_string) {
                input.args.insert("root".to_string(), path);
            }
        }
        if input.get("limit").is_none() {
            if let Some(max_results) = input.get("max_results").map(str::to_string) {
                input.args.insert("limit".to_string(), max_results);
            }
        }
        SearchTextTool.execute(input)
    }
}

fn search_dir(
    root: &Path,
    current: &Path,
    query: &str,
    limit: usize,
    matches: &mut Vec<String>,
) -> AppResult<()> {
    if matches.len() >= limit {
        return Ok(());
    }

    let mut entries = fs::read_dir(current)?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .collect::<Vec<PathBuf>>();
    entries.sort();

    for path in entries {
        if matches.len() >= limit {
            break;
        }

        let name = path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("");
        if should_skip(name) {
            continue;
        }

        if path.is_dir() {
            search_dir(root, &path, query, limit, matches)?;
            continue;
        }

        let Ok(content) = fs::read_to_string(&path) else {
            continue;
        };

        for (index, line) in content.lines().enumerate() {
            if matches.len() >= limit {
                break;
            }

            if line.contains(query) {
                let display = path.strip_prefix(root).unwrap_or(&path).display();
                matches.push(format!("{display}:{}: {}", index + 1, line.trim()));
            }
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
            "deepseek-search-text-{name}-{}-{suffix}",
            std::process::id()
        ))
    }

    #[test]
    fn grep_files_maps_deepseek_tui_args_to_search_text() {
        let root = temp_root("grep-alias");
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/lib.rs"), "pub fn target_symbol() {}\n").unwrap();

        let output = GrepFilesTool
            .execute(
                ToolInput::new()
                    .with_arg("pattern", "target_symbol")
                    .with_arg("path", root.display().to_string())
                    .with_arg("max_results", "5"),
            )
            .unwrap();

        assert!(output.summary.contains("src/lib.rs:1"));
        assert!(output.summary.contains("target_symbol"));
    }
}
