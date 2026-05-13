use std::io::Write;

use crate::error::{app_error, AppResult};
use crate::tools::types::{Tool, ToolInput, ToolOutput};

const TITLE_CAP: usize = 80;
const BODY_CAP: usize = 200;

pub struct NotifyTool;

impl Tool for NotifyTool {
    fn name(&self) -> &str {
        "notify"
    }

    fn execute(&self, input: ToolInput) -> AppResult<ToolOutput> {
        let title_raw = input
            .get("title")
            .ok_or_else(|| app_error("notify requires `title`"))?;
        let title = truncate_chars(title_raw, TITLE_CAP).trim().to_string();
        if title.is_empty() {
            return Err(app_error("notify title must not be empty"));
        }
        let body = input
            .get("body")
            .map(|value| truncate_chars(value, BODY_CAP).trim().to_string())
            .unwrap_or_default();
        if notify_enabled() {
            let _ = std::io::stderr().write_all(b"\x07");
            let _ = std::io::stderr().flush();
        }
        let summary = if body.is_empty() {
            format!("notified: {title}")
        } else {
            format!("notified: {title} - {body}")
        };
        Ok(ToolOutput { summary })
    }
}

fn notify_enabled() -> bool {
    !matches!(
        std::env::var("DSCODE_NOTIFY").ok().as_deref(),
        Some("0") | Some("off") | Some("false") | Some("OFF") | Some("FALSE")
    )
}

fn truncate_chars(value: &str, cap: usize) -> String {
    value.chars().take(cap).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, OnceLock};

    fn env_lock() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(())).lock().unwrap()
    }

    fn execute_quiet(input: ToolInput) -> AppResult<ToolOutput> {
        let _guard = env_lock();
        std::env::set_var("DSCODE_NOTIFY", "off");
        let result = NotifyTool.execute(input);
        std::env::remove_var("DSCODE_NOTIFY");
        result
    }

    #[test]
    fn notify_requires_title() {
        let error = execute_quiet(ToolInput::new()).unwrap_err();

        assert!(error.to_string().contains("title"));
    }

    #[test]
    fn notify_rejects_empty_title_after_trim() {
        let error = execute_quiet(ToolInput::new().with_arg("title", "   ")).unwrap_err();

        assert!(error.to_string().contains("must not be empty"));
    }

    #[test]
    fn notify_truncates_title_by_chars() {
        let title = "x".repeat(TITLE_CAP + 20);
        let output = execute_quiet(ToolInput::new().with_arg("title", title)).unwrap();

        assert_eq!(output.summary.matches('x').count(), TITLE_CAP);
    }

    #[test]
    fn notify_accepts_optional_body() {
        let output = execute_quiet(
            ToolInput::new()
                .with_arg("title", "done")
                .with_arg("body", "tests pass"),
        )
        .unwrap();

        assert!(output.summary.contains("done - tests pass"));
    }

    #[test]
    fn truncate_chars_does_not_split_multibyte_chars() {
        let title = "我".repeat(30);
        let output = execute_quiet(ToolInput::new().with_arg("title", title.clone())).unwrap();

        assert!(output.summary.contains(&title));
    }
}
