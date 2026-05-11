use std::path::Path;

use crate::error::AppResult;
use crate::tools::types::{Tool, ToolInput, ToolOutput};

pub struct DiagnosticsTool;

impl Tool for DiagnosticsTool {
    fn name(&self) -> &str {
        "diagnostics"
    }

    fn execute(&self, input: ToolInput) -> AppResult<ToolOutput> {
        let cwd = input.get("cwd").unwrap_or(".");
        let paths = input.get("paths").map(split_paths).unwrap_or_default();
        let report = crate::language::diagnostics::run_diagnostics(Path::new(cwd), &paths);
        Ok(ToolOutput {
            summary: report.render_text(),
        })
    }
}

pub fn split_paths(value: &str) -> Vec<String> {
    value
        .split(|ch| matches!(ch, '\n' | ',' | ';'))
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(str::to_string)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_paths_accepts_common_delimiters() {
        assert_eq!(
            split_paths("src/lib.rs, src/main.rs\nREADME.md;docs/a.md"),
            vec![
                "src/lib.rs".to_string(),
                "src/main.rs".to_string(),
                "README.md".to_string(),
                "docs/a.md".to_string(),
            ]
        );
    }
}
