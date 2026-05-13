use crate::error::{app_error, AppResult};
use crate::tools::run_shell::RunShellTool;
use crate::tools::types::{Tool, ToolInput, ToolOutput};
use crate::util::cancel::CancellationCheck;
use std::path::Path;

pub struct RunTestsTool;

impl Tool for RunTestsTool {
    fn name(&self) -> &str {
        "run_tests"
    }

    fn execute(&self, input: ToolInput) -> AppResult<ToolOutput> {
        self.execute_inner(input, None)
    }

    fn execute_with_cancel(
        &self,
        input: ToolInput,
        cancel_check: Option<&mut dyn CancellationCheck>,
    ) -> AppResult<ToolOutput> {
        self.execute_inner(input, cancel_check)
    }
}

impl RunTestsTool {
    fn execute_inner(
        &self,
        input: ToolInput,
        cancel_check: Option<&mut dyn CancellationCheck>,
    ) -> AppResult<ToolOutput> {
        let cwd = input.get("cwd").unwrap_or(".");
        let command = render_run_tests_command(&input)?;
        let shell_input = ToolInput::new()
            .with_arg("cwd", cwd)
            .with_arg("command", command);
        RunShellTool.execute_with_cancel(shell_input, cancel_check)
    }
}

pub(crate) fn render_run_tests_command(input: &ToolInput) -> AppResult<String> {
    let cwd = input.get("cwd").unwrap_or(".");
    let all_features = input
        .get("all_features")
        .is_some_and(|value| matches!(value, "1" | "true" | "TRUE" | "yes" | "on"));
    let extra_args = input.get("args").unwrap_or("").trim();
    validate_safe_arg_tail(extra_args)?;

    let mut command = if let Some(command) = input
        .get("command")
        .filter(|value| !value.trim().is_empty())
    {
        normalize_test_command(command)?
    } else {
        infer_test_command(Path::new(cwd))?
    };
    if all_features && command == "cargo test" {
        command.push_str(" --all-features");
    }
    if !extra_args.is_empty() {
        command.push(' ');
        command.push_str(extra_args);
    }
    Ok(command)
}

fn normalize_test_command(command: &str) -> AppResult<String> {
    let command = command.trim();
    let allowed = [
        "cargo test",
        "go test",
        "pytest",
        "python -m pytest",
        "node --test",
        "pnpm test",
        "npm test",
        "mvn test",
        "gradle test",
    ];
    for prefix in allowed {
        if command == prefix {
            return Ok(command.to_string());
        }
        if let Some(tail) = command
            .strip_prefix(prefix)
            .and_then(|value| value.strip_prefix(' '))
        {
            validate_safe_arg_tail(tail)?;
            return Ok(command.to_string());
        }
    }
    Err(app_error(format!(
        "run_tests command must start with a supported test command, got `{command}`"
    )))
}

fn infer_test_command(cwd: &Path) -> AppResult<String> {
    if cwd.join("Cargo.toml").exists() {
        Ok("cargo test".to_string())
    } else if cwd.join("go.mod").exists() {
        Ok("go test ./...".to_string())
    } else if cwd.join("pnpm-lock.yaml").exists() {
        Ok("pnpm test".to_string())
    } else if cwd.join("package.json").exists() {
        Ok("npm test".to_string())
    } else if cwd.join("pyproject.toml").exists()
        || cwd.join("pytest.ini").exists()
        || cwd.join("tests").is_dir()
    {
        Ok("pytest".to_string())
    } else {
        Err(app_error(
            "run_tests could not infer a test command; provide command",
        ))
    }
}

fn validate_safe_arg_tail(args: &str) -> AppResult<()> {
    if args.chars().any(|ch| {
        matches!(
            ch,
            ';' | '|' | '&' | '<' | '>' | '$' | '`' | '(' | ')' | '{' | '}' | '\n' | '\r'
        )
    }) {
        return Err(app_error(
            "run_tests arguments contain shell metacharacters",
        ));
    }
    for token in args.split_whitespace() {
        if !token.chars().all(|ch| {
            ch.is_ascii_alphanumeric()
                || matches!(
                    ch,
                    '_' | '-' | '.' | '/' | ':' | '=' | ',' | '@' | '+' | '%'
                )
        }) {
            return Err(app_error(format!(
                "run_tests argument `{token}` is not supported"
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    fn temp_root(name: &str) -> std::path::PathBuf {
        let suffix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "deepseek-run-tests-{name}-{}-{suffix}",
            std::process::id()
        ))
    }

    fn cargo_available() -> bool {
        Command::new("cargo")
            .arg("--version")
            .output()
            .map(|output| output.status.success())
            .unwrap_or(false)
    }

    #[test]
    fn run_tests_infers_cargo_command_and_rejects_unsafe_tail() {
        let root = temp_root("infer");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("Cargo.toml"), "[package]\nname = \"demo\"\n").unwrap();

        let command = render_run_tests_command(
            &ToolInput::new()
                .with_arg("cwd", root.display().to_string())
                .with_arg("all_features", "true")
                .with_arg("args", "--quiet"),
        )
        .unwrap();
        assert_eq!(command, "cargo test --all-features --quiet");

        let error =
            render_run_tests_command(&ToolInput::new().with_arg("command", "cargo test; rm -rf /"))
                .unwrap_err();
        assert!(error.to_string().contains("supported test command"));
    }

    #[test]
    fn run_tests_executes_small_cargo_project_when_available() {
        if !cargo_available() {
            return;
        }
        let root = temp_root("cargo");
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(
            root.join("Cargo.toml"),
            "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .unwrap();
        std::fs::write(
            root.join("src/lib.rs"),
            "pub fn add(left: i32, right: i32) -> i32 { left + right }\n#[test]\nfn adds() { assert_eq!(add(2, 3), 5); }\n",
        )
        .unwrap();

        let output = RunTestsTool
            .execute(
                ToolInput::new()
                    .with_arg("cwd", root.display().to_string())
                    .with_arg("args", "--quiet"),
            )
            .unwrap();

        assert!(output.summary.contains("meta.command_kind=test"));
        assert!(output.summary.contains("meta.result=ok"));
    }
}
