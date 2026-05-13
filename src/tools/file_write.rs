use std::fs;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};

use crate::config::types::{AppConfig, ModelConfig};
use crate::error::{app_error, AppResult};
use crate::tools::types::{Tool, ToolInput, ToolOutput};
use crate::util::json::{
    json_as_array, json_as_object, json_as_string, json_escape, parse_json_value,
};

pub struct WriteFileTool;
pub struct EditFileTool;
pub struct FimEditTool {
    model: ModelConfig,
}

impl FimEditTool {
    pub fn new(config: &AppConfig) -> Self {
        Self {
            model: config.model.clone(),
        }
    }
}

impl Tool for WriteFileTool {
    fn name(&self) -> &str {
        "write_file"
    }

    fn execute(&self, input: ToolInput) -> AppResult<ToolOutput> {
        let raw_path = required_nonempty(&input, "path", "write_file")?;
        let content = input
            .get("content")
            .ok_or_else(|| app_error("write_file requires `content`"))?;
        let base = workspace_base(&input);
        let target = safe_workspace_path(&base, &raw_path, "write_file")?;
        refuse_symlink_components(&base, &raw_path, "write_file")?;
        if let Ok(metadata) = fs::symlink_metadata(&target) {
            if metadata.file_type().is_symlink() {
                return Err(app_error(format!(
                    "write_file refuses symlink target: {}",
                    target.display()
                )));
            }
            if metadata.is_dir() {
                return Err(app_error(format!(
                    "write_file target is a directory: {}",
                    target.display()
                )));
            }
        }
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        let existed = target.exists();
        fs::write(&target, content)?;
        Ok(ToolOutput {
            summary: format!(
                "{} {} bytes to {}",
                if existed { "Wrote" } else { "Created" },
                content.len(),
                raw_path
            ),
        })
    }
}

impl Tool for EditFileTool {
    fn name(&self) -> &str {
        "edit_file"
    }

    fn execute(&self, input: ToolInput) -> AppResult<ToolOutput> {
        let raw_path = required_nonempty(&input, "path", "edit_file")?;
        let search = required_nonempty(&input, "search", "edit_file")?;
        let replace = input
            .get("replace")
            .ok_or_else(|| app_error("edit_file requires `replace`"))?;
        if search == replace {
            return Err(app_error(
                "search and replace are identical, no change intended",
            ));
        }
        let base = workspace_base(&input);
        let target = safe_workspace_path(&base, &raw_path, "edit_file")?;
        refuse_symlink_components(&base, &raw_path, "edit_file")?;
        let metadata = fs::symlink_metadata(&target).map_err(|error| {
            app_error(format!(
                "edit_file failed to inspect `{}`: {error}",
                target.display()
            ))
        })?;
        if metadata.file_type().is_symlink() {
            return Err(app_error(format!(
                "edit_file refuses symlink target: {}",
                target.display()
            )));
        }
        if metadata.is_dir() {
            return Err(app_error(format!(
                "edit_file target is a directory: {}",
                target.display()
            )));
        }

        let contents = fs::read_to_string(&target)?;
        let count = contents.matches(&search).count();
        if count == 0 {
            return Err(app_error(format!(
                "Search string not found in {}",
                target.display()
            )));
        }
        let updated = contents.replace(&search, replace);
        fs::write(&target, updated)?;
        Ok(ToolOutput {
            summary: format!("Replaced {count} occurrence(s) in {raw_path}"),
        })
    }
}

impl Tool for FimEditTool {
    fn name(&self) -> &str {
        "fim_edit"
    }

    fn execute(&self, input: ToolInput) -> AppResult<ToolOutput> {
        let raw_path = required_nonempty(&input, "path", "fim_edit")?;
        let prefix_anchor = required_nonempty(&input, "prefix_anchor", "fim_edit")?;
        let suffix_anchor = required_nonempty(&input, "suffix_anchor", "fim_edit")?;
        let max_tokens = parse_u32_arg(&input, "max_tokens", 1024).clamp(1, 16_384);
        let base = workspace_base(&input);
        let target = safe_workspace_path(&base, &raw_path, "fim_edit")?;
        refuse_symlink_components(&base, &raw_path, "fim_edit")?;
        let metadata = fs::symlink_metadata(&target).map_err(|error| {
            app_error(format!(
                "fim_edit failed to inspect `{}`: {error}",
                target.display()
            ))
        })?;
        if metadata.file_type().is_symlink() {
            return Err(app_error(format!(
                "fim_edit refuses symlink target: {}",
                target.display()
            )));
        }
        if metadata.is_dir() {
            return Err(app_error(format!(
                "fim_edit target is a directory: {}",
                target.display()
            )));
        }

        let content = fs::read_to_string(&target)?;
        let prefix_pos = content.find(&prefix_anchor).ok_or_else(|| {
            app_error(format!(
                "Prefix anchor not found in file `{raw_path}`: `{prefix_anchor}`"
            ))
        })?;
        let prefix_end = prefix_pos + prefix_anchor.len();
        let suffix_offset = content[prefix_end..].find(&suffix_anchor).ok_or_else(|| {
            app_error(format!(
                "Suffix anchor not found after prefix anchor in `{raw_path}`: `{suffix_anchor}`"
            ))
        })?;
        let suffix_start = prefix_end + suffix_offset;
        let fim_prompt = &content[..prefix_end];
        let fim_suffix = &content[suffix_start..];
        let generated_text = if let Some(generated) = input.get("generated_text") {
            generated.to_string()
        } else {
            call_fim_completion(&self.model, &input, fim_prompt, fim_suffix, max_tokens)?
        };

        let generated_len = generated_text.len();
        let new_content = format!("{fim_prompt}{generated_text}{fim_suffix}");
        fs::write(&target, new_content)?;

        Ok(ToolOutput {
            summary: format!(
                "{{\"success\":true,\"path\":\"{}\",\"generated_text\":\"{}\",\"prefix_end\":{},\"suffix_start\":{},\"generated_len\":{},\"message\":\"FIM edit applied to `{}`. Generated {} chars between prefix_anchor end byte {} and suffix_anchor start byte {}.\"}}",
                json_escape(&raw_path),
                json_escape(&generated_text),
                prefix_end,
                suffix_start,
                generated_len,
                json_escape(&raw_path),
                generated_len,
                prefix_end,
                suffix_start,
            ),
        })
    }
}

fn required_nonempty(input: &ToolInput, key: &str, tool_name: &str) -> AppResult<String> {
    input
        .get(key)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .ok_or_else(|| app_error(format!("{tool_name} requires `{key}`")))
}

fn parse_u32_arg(input: &ToolInput, key: &str, default: u32) -> u32 {
    input
        .get(key)
        .and_then(|value| value.trim().parse::<u32>().ok())
        .unwrap_or(default)
}

fn call_fim_completion(
    config: &ModelConfig,
    input: &ToolInput,
    prompt: &str,
    suffix: &str,
    max_tokens: u32,
) -> AppResult<String> {
    let api_key_env = input.get("api_key_env").unwrap_or(&config.api_key_env);
    let api_key = std::env::var(api_key_env).map_err(|_| {
        app_error(format!(
            "fim_edit requires `{api_key_env}` or a `generated_text` override for offline use"
        ))
    })?;
    let model = input.get("model").unwrap_or(&config.model);
    let endpoint = fim_endpoint(input.get("base_url").unwrap_or(&config.base_url));
    let body = format!(
        "{{\"model\":\"{}\",\"prompt\":\"{}\",\"suffix\":\"{}\",\"max_tokens\":{}}}",
        json_escape(model),
        json_escape(prompt),
        json_escape(suffix),
        max_tokens
    );
    let auth = format!("Authorization: Bearer {api_key}");
    let mut child = Command::new("curl")
        .args([
            "-sS",
            "--max-time",
            "60",
            "-H",
            "Content-Type: application/json",
            "-H",
            &auth,
            "-d",
            "@-",
            "--",
            &endpoint,
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| app_error(format!("could not invoke curl for fim_edit: {error}")))?;
    {
        use std::io::Write;
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| app_error("curl produced no stdin pipe for fim_edit"))?;
        stdin.write_all(body.as_bytes()).map_err(|error| {
            app_error(format!("failed to write fim_edit request body: {error}"))
        })?;
    }
    let output = child.wait_with_output()?;
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if !output.status.success() {
        return Err(app_error(format!(
            "fim_edit FIM API call failed: {}",
            if stderr.is_empty() {
                output.status.to_string()
            } else {
                stderr
            }
        )));
    }
    parse_fim_completion_text(&stdout)
}

fn fim_endpoint(base_url: &str) -> String {
    let base = base_url.trim_end_matches('/');
    if base.ends_with("/beta") {
        format!("{base}/completions")
    } else {
        format!("{base}/beta/completions")
    }
}

fn parse_fim_completion_text(body: &str) -> AppResult<String> {
    let parsed = parse_json_value(body.trim())?;
    let root = json_as_object(&parsed)
        .ok_or_else(|| app_error("fim_edit response root must be a JSON object"))?;
    if let Some(error) = root.get("error").and_then(json_as_object) {
        let message = error
            .get("message")
            .and_then(json_as_string)
            .unwrap_or("unknown FIM API error");
        return Err(app_error(format!(
            "fim_edit FIM API call failed: {message}"
        )));
    }
    let choices = root
        .get("choices")
        .and_then(json_as_array)
        .ok_or_else(|| app_error("fim_edit response missing `choices` array"))?;
    let first = choices
        .first()
        .and_then(json_as_object)
        .ok_or_else(|| app_error("fim_edit response missing first choice"))?;
    if let Some(text) = first.get("text").and_then(json_as_string) {
        return Ok(text.to_string());
    }
    if let Some(content) = first
        .get("message")
        .and_then(json_as_object)
        .and_then(|message| message.get("content"))
        .and_then(json_as_string)
    {
        return Ok(content.to_string());
    }
    Err(app_error("fim_edit response choice missing generated text"))
}

fn workspace_base(input: &ToolInput) -> PathBuf {
    input
        .get("cwd")
        .or_else(|| input.get("workspace"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

fn safe_workspace_path(base: &Path, raw_path: &str, tool_name: &str) -> AppResult<PathBuf> {
    let path = Path::new(raw_path);
    if raw_path.trim().is_empty() || path.is_absolute() {
        return Err(app_error(format!(
            "unsafe {tool_name} path outside workspace: {raw_path}"
        )));
    }
    for component in path.components() {
        match component {
            Component::Normal(_) | Component::CurDir => {}
            Component::ParentDir | Component::Prefix(_) | Component::RootDir => {
                return Err(app_error(format!(
                    "unsafe {tool_name} path outside workspace: {raw_path}"
                )));
            }
        }
    }
    Ok(base.join(path))
}

fn refuse_symlink_components(base: &Path, raw_path: &str, tool_name: &str) -> AppResult<()> {
    let mut current = base.to_path_buf();
    for component in Path::new(raw_path).components() {
        let Component::Normal(part) = component else {
            continue;
        };
        current.push(part);
        if let Ok(metadata) = fs::symlink_metadata(&current) {
            if metadata.file_type().is_symlink() {
                return Err(app_error(format!(
                    "{tool_name} refuses symlink path component: {}",
                    current.display()
                )));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_root(label: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "deepseek-file-write-{label}-{}-{nanos}",
            std::process::id()
        ));
        fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn write_file_creates_parent_directories() {
        let root = temp_root("write");
        let output = WriteFileTool
            .execute(
                ToolInput::new()
                    .with_arg("cwd", root.display().to_string())
                    .with_arg("path", "nested/out.txt")
                    .with_arg("content", "hello\n"),
            )
            .unwrap();

        assert!(output.summary.contains("Created 6 bytes"));
        assert_eq!(
            fs::read_to_string(root.join("nested/out.txt")).unwrap(),
            "hello\n"
        );
    }

    #[test]
    fn write_file_refuses_parent_escape() {
        let root = temp_root("escape");
        let error = WriteFileTool
            .execute(
                ToolInput::new()
                    .with_arg("cwd", root.display().to_string())
                    .with_arg("path", "../escape.txt")
                    .with_arg("content", "nope"),
            )
            .unwrap_err();

        assert!(error.to_string().contains("unsafe write_file path"));
    }

    #[test]
    fn edit_file_replaces_all_exact_matches() {
        let root = temp_root("edit");
        fs::write(root.join("note.txt"), "hello world hello").unwrap();

        let output = EditFileTool
            .execute(
                ToolInput::new()
                    .with_arg("cwd", root.display().to_string())
                    .with_arg("path", "note.txt")
                    .with_arg("search", "hello")
                    .with_arg("replace", "hi"),
            )
            .unwrap();

        assert!(output.summary.contains("2 occurrence(s)"));
        assert_eq!(
            fs::read_to_string(root.join("note.txt")).unwrap(),
            "hi world hi"
        );
    }

    #[test]
    fn edit_file_rejects_missing_search() {
        let root = temp_root("missing");
        fs::write(root.join("note.txt"), "alpha").unwrap();

        let error = EditFileTool
            .execute(
                ToolInput::new()
                    .with_arg("cwd", root.display().to_string())
                    .with_arg("path", "note.txt")
                    .with_arg("search", "beta")
                    .with_arg("replace", "gamma"),
            )
            .unwrap_err();

        assert!(error.to_string().contains("Search string not found"));
    }

    #[test]
    fn edit_file_rejects_identical_search_and_replace() {
        let root = temp_root("same");
        fs::write(root.join("note.txt"), "alpha").unwrap();

        let error = EditFileTool
            .execute(
                ToolInput::new()
                    .with_arg("cwd", root.display().to_string())
                    .with_arg("path", "note.txt")
                    .with_arg("search", "alpha")
                    .with_arg("replace", "alpha"),
            )
            .unwrap_err();

        assert!(error.to_string().contains("identical"));
    }

    #[test]
    fn fim_edit_replaces_middle_between_anchors_with_generated_text() {
        let root = temp_root("fim");
        fs::write(root.join("note.txt"), "fn main() {\n    old();\n}\n").unwrap();

        let output = FimEditTool::new(&AppConfig::default())
            .execute(
                ToolInput::new()
                    .with_arg("cwd", root.display().to_string())
                    .with_arg("path", "note.txt")
                    .with_arg("prefix_anchor", "fn main() {\n")
                    .with_arg("suffix_anchor", "}\n")
                    .with_arg("generated_text", "    new();\n"),
            )
            .unwrap();

        assert!(output.summary.contains("\"success\":true"));
        assert!(output.summary.contains("\"generated_len\":11"));
        assert_eq!(
            fs::read_to_string(root.join("note.txt")).unwrap(),
            "fn main() {\n    new();\n}\n"
        );
    }

    #[test]
    fn fim_edit_rejects_missing_suffix_after_prefix() {
        let root = temp_root("fim-missing");
        fs::write(root.join("note.txt"), "alpha\nbeta\n").unwrap();

        let error = FimEditTool::new(&AppConfig::default())
            .execute(
                ToolInput::new()
                    .with_arg("cwd", root.display().to_string())
                    .with_arg("path", "note.txt")
                    .with_arg("prefix_anchor", "alpha\n")
                    .with_arg("suffix_anchor", "gamma\n")
                    .with_arg("generated_text", "middle\n"),
            )
            .unwrap_err();

        assert!(error.to_string().contains("Suffix anchor not found"));
    }

    #[test]
    fn fim_edit_requires_api_key_without_generated_text() {
        let root = temp_root("fim-key");
        fs::write(root.join("note.txt"), "a\nb\n").unwrap();

        let error = FimEditTool::new(&AppConfig::default())
            .execute(
                ToolInput::new()
                    .with_arg("cwd", root.display().to_string())
                    .with_arg("path", "note.txt")
                    .with_arg("prefix_anchor", "a\n")
                    .with_arg("suffix_anchor", "b\n")
                    .with_arg("api_key_env", "DSCODE_TEST_MISSING_FIM_KEY"),
            )
            .unwrap_err();

        assert!(error.to_string().contains("DSCODE_TEST_MISSING_FIM_KEY"));
    }

    #[test]
    fn parse_fim_completion_text_reads_openai_completion_shape() {
        let text =
            parse_fim_completion_text(r#"{"choices":[{"text":"generated middle"}]}"#).unwrap();
        assert_eq!(text, "generated middle");
    }
}
