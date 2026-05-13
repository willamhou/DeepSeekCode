use crate::error::{app_error, AppResult};
use crate::tools::types::{Tool, ToolInput, ToolOutput};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

const DEFAULT_LIMIT: usize = 20;
const MAX_LIMIT: usize = 100;

pub struct FileSearchTool;

#[derive(Debug, Clone, PartialEq, Eq)]
struct FileMatch {
    score: usize,
    path: String,
}

impl Tool for FileSearchTool {
    fn name(&self) -> &str {
        "file_search"
    }

    fn execute(&self, input: ToolInput) -> AppResult<ToolOutput> {
        let query = input
            .get("query")
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| app_error("file_search requires a query"))?;
        let root = input
            .get("path")
            .or_else(|| input.get("root"))
            .unwrap_or(".");
        let limit = input
            .get("limit")
            .or_else(|| input.get("max_results"))
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(DEFAULT_LIMIT)
            .clamp(1, MAX_LIMIT);
        let extensions = parse_extensions(input.get("extensions"));

        let mut matches = Vec::new();
        visit_files(
            Path::new(root),
            Path::new(root),
            &query.to_lowercase(),
            &extensions,
            &mut matches,
        )?;
        matches.sort_by(|left, right| {
            right
                .score
                .cmp(&left.score)
                .then_with(|| left.path.cmp(&right.path))
        });
        matches.truncate(limit);

        Ok(ToolOutput {
            summary: if matches.is_empty() {
                format!("No file matches for `{query}`.")
            } else {
                matches
                    .into_iter()
                    .map(|item| format!("score={} {}", item.score, item.path))
                    .collect::<Vec<_>>()
                    .join("\n")
            },
        })
    }
}

fn visit_files(
    root: &Path,
    current: &Path,
    query: &str,
    extensions: &BTreeSet<String>,
    matches: &mut Vec<FileMatch>,
) -> AppResult<()> {
    let mut entries = fs::read_dir(current)?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .collect::<Vec<PathBuf>>();
    entries.sort();

    for path in entries {
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

        if metadata.is_dir() {
            visit_files(root, &path, query, extensions, matches)?;
            continue;
        }
        if !extension_allowed(&path, extensions) {
            continue;
        }

        let display = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .display()
            .to_string();
        if let Some(score) = score_file_match(query, name, &display) {
            matches.push(FileMatch {
                score,
                path: display,
            });
        }
    }

    Ok(())
}

fn parse_extensions(value: Option<&str>) -> BTreeSet<String> {
    value
        .unwrap_or("")
        .split(|ch: char| ch == ',' || ch == ';' || ch.is_whitespace())
        .filter_map(|part| {
            let trimmed = part.trim().trim_start_matches('.').to_lowercase();
            (!trimmed.is_empty()).then_some(trimmed)
        })
        .collect()
}

fn extension_allowed(path: &Path, extensions: &BTreeSet<String>) -> bool {
    if extensions.is_empty() {
        return true;
    }
    path.extension()
        .and_then(|value| value.to_str())
        .map(|value| extensions.contains(&value.to_lowercase()))
        .unwrap_or(false)
}

fn score_file_match(query: &str, file_name: &str, display_path: &str) -> Option<usize> {
    let file_name = file_name.to_lowercase();
    let display_path = display_path.to_lowercase();
    if file_name == query {
        Some(100)
    } else if file_name.contains(query) {
        Some(80)
    } else if display_path.contains(query) {
        Some(60)
    } else if ordered_chars_match(query, &file_name) {
        Some(40)
    } else if ordered_chars_match(query, &display_path) {
        Some(20)
    } else {
        None
    }
}

fn ordered_chars_match(query: &str, value: &str) -> bool {
    let mut chars = query.chars();
    let Some(mut needle) = chars.next() else {
        return false;
    };
    for ch in value.chars() {
        if ch == needle {
            match chars.next() {
                Some(next) => needle = next,
                None => return true,
            }
        }
    }
    false
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
            "deepseek-file-search-{name}-{}-{suffix}",
            std::process::id()
        ))
    }

    #[test]
    fn file_search_finds_filename_matches_with_extension_filter() {
        let root = temp_root("find");
        std::fs::create_dir_all(root.join("src/tools")).unwrap();
        std::fs::write(
            root.join("src/tools/search_text.rs"),
            "pub struct SearchText;\n",
        )
        .unwrap();
        std::fs::write(root.join("src/tools/search_text.md"), "# SearchText\n").unwrap();

        let output = FileSearchTool
            .execute(
                ToolInput::new()
                    .with_arg("query", "search")
                    .with_arg("path", root.display().to_string())
                    .with_arg("extensions", "rs")
                    .with_arg("limit", "10"),
            )
            .unwrap();

        assert!(output.summary.contains("src/tools/search_text.rs"));
        assert!(!output.summary.contains("src/tools/search_text.md"));
    }

    #[test]
    fn file_search_supports_ordered_character_matches() {
        assert_eq!(
            score_file_match("stx", "search_text.rs", "src/search_text.rs"),
            Some(40)
        );
        assert_eq!(
            score_file_match("zzz", "search_text.rs", "src/search_text.rs"),
            None
        );
    }
}
