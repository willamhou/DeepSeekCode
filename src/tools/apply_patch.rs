use crate::error::AppResult;
use crate::error::app_error;
use crate::tools::types::{Tool, ToolInput, ToolOutput};
use std::fs;
use std::path::Path;

pub struct ApplyPatchTool;

impl Tool for ApplyPatchTool {
    fn name(&self) -> &'static str {
        "apply_patch"
    }

    fn execute(&self, input: ToolInput) -> AppResult<ToolOutput> {
        let path = input
            .get("path")
            .ok_or_else(|| app_error("apply_patch requires a path"))?;
        let find = input
            .get("find")
            .ok_or_else(|| app_error("apply_patch requires a find string"))?;
        let replace = input
            .get("replace")
            .ok_or_else(|| app_error("apply_patch requires a replace string"))?;
        let replace_all = input.get("replace_all").unwrap_or("false") == "true";
        let path = Path::new(path);

        if path.is_dir() {
            return Err(app_error("apply_patch path points to a directory"));
        }

        let original = fs::read_to_string(path)?;
        let updated = apply_replacement(&original, find, replace, replace_all)?;
        fs::write(path, updated)?;

        Ok(ToolOutput {
            summary: format!(
                "Updated {} using {} replacement mode.",
                path.display(),
                if replace_all { "global" } else { "single" }
            ),
        })
    }
}

fn apply_replacement(
    original: &str,
    find: &str,
    replace: &str,
    replace_all: bool,
) -> AppResult<String> {
    if find.is_empty() {
        return Err(app_error("find string cannot be empty"));
    }

    if !original.contains(find) {
        return Err(app_error("find string not found in target file"));
    }

    let updated = if replace_all {
        original.replace(find, replace)
    } else {
        original.replacen(find, replace, 1)
    };

    Ok(updated)
}

#[cfg(test)]
mod tests {
    use super::apply_replacement;

    #[test]
    fn replaces_first_occurrence_only() {
        let updated = apply_replacement("a b a", "a", "x", false).unwrap();
        assert_eq!(updated, "x b a");
    }

    #[test]
    fn replaces_all_occurrences() {
        let updated = apply_replacement("a b a", "a", "x", true).unwrap();
        assert_eq!(updated, "x b x");
    }

    #[test]
    fn errors_when_find_is_missing() {
        let error = apply_replacement("hello", "missing", "x", false).unwrap_err();
        assert!(error.to_string().contains("not found"));
    }
}
