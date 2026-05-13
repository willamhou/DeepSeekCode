use std::fs;
use std::io::Write;
use std::path::PathBuf;

use crate::core::memory::append_user_memory;
use crate::error::{app_error, AppResult};
use crate::tools::types::{Tool, ToolInput, ToolOutput};

pub struct NoteTool {
    path: PathBuf,
}

pub struct RememberTool {
    path: PathBuf,
}

impl NoteTool {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }
}

impl RememberTool {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }
}

impl Tool for NoteTool {
    fn name(&self) -> &str {
        "note"
    }

    fn execute(&self, input: ToolInput) -> AppResult<ToolOutput> {
        let content = input
            .get("content")
            .or_else(|| input.get("note"))
            .ok_or_else(|| app_error("note requires `content`"))?
            .trim();
        if content.is_empty() {
            return Err(app_error("note content must not be empty"));
        }
        if let Some(parent) = self.path.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent)?;
            }
        }
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        writeln!(file, "\n---\n{content}")?;
        Ok(ToolOutput {
            summary: format!("Note appended to {}", self.path.display()),
        })
    }
}

impl Tool for RememberTool {
    fn name(&self) -> &str {
        "remember"
    }

    fn execute(&self, input: ToolInput) -> AppResult<ToolOutput> {
        let note = input
            .get("note")
            .or_else(|| input.get("content"))
            .ok_or_else(|| app_error("remember requires `note`"))?;
        let remembered = append_user_memory(&self.path, note)?;
        Ok(ToolOutput {
            summary: format!("remembered: {remembered}"),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_root(name: &str) -> PathBuf {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "deepseek-notes-{name}-{}-{suffix}",
            std::process::id()
        ))
    }

    #[test]
    fn note_appends_to_configured_path() {
        let root = temp_root("append");
        let path = root.join("notes.md");
        let tool = NoteTool::new(path.clone());

        let output = tool
            .execute(ToolInput::new().with_arg("content", "keep release notes short"))
            .unwrap();
        let content = fs::read_to_string(&path).unwrap();

        assert!(output.summary.contains(&path.display().to_string()));
        assert!(content.contains("---\nkeep release notes short"));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn note_rejects_empty_content() {
        let root = temp_root("empty-note");
        let path = root.join("notes.md");
        let tool = NoteTool::new(path);

        let error = tool
            .execute(ToolInput::new().with_arg("content", "  "))
            .unwrap_err();

        assert!(error.to_string().contains("must not be empty"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn remember_appends_to_configured_path() {
        let root = temp_root("remember");
        let path = root.join("memory.md");
        let tool = RememberTool::new(path.clone());

        let output = tool
            .execute(ToolInput::new().with_arg("note", "# prefer cargo fmt"))
            .unwrap();
        let content = fs::read_to_string(&path).unwrap();

        assert_eq!(output.summary, "remembered: prefer cargo fmt");
        assert!(content.contains("prefer cargo fmt"));
        assert!(!content.contains("# prefer cargo fmt"));

        let _ = fs::remove_dir_all(root);
    }
}
