use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::config::types::{AppConfig, VisionConfig};
use crate::error::{app_error, AppResult};
use crate::tools::types::{Tool, ToolInput, ToolOutput};
use crate::util::json::{
    json_as_array, json_as_object, json_as_string, json_escape, parse_json_value,
};
use crate::workspace_trust::resolve_workspace_path;

pub struct ImageAnalyzeTool {
    vision: VisionConfig,
}

impl ImageAnalyzeTool {
    pub fn new(config: &AppConfig) -> Self {
        Self {
            vision: config.vision.clone(),
        }
    }
}

impl Tool for ImageAnalyzeTool {
    fn name(&self) -> &str {
        "image_analyze"
    }

    fn execute(&self, input: ToolInput) -> AppResult<ToolOutput> {
        let raw_path = required_nonempty_any(&input, &["image_path", "path"], "image_analyze")?;
        let prompt = input
            .get("prompt")
            .filter(|value| !value.trim().is_empty())
            .unwrap_or("Describe this image in detail.");
        let base = workspace_base(&input);
        let image = safe_workspace_path(&base, &raw_path, "image_analyze")?;
        if !image.exists() {
            return Err(app_error(format!(
                "image_analyze image_path does not exist: {}",
                image.display()
            )));
        }
        if image.is_dir() {
            return Err(app_error(format!(
                "image_analyze image_path is a directory: {}",
                image.display()
            )));
        }
        let mime_type = detect_mime_type(&image)?;
        let image_data = encode_base64(&fs::read(&image).map_err(|error| {
            app_error(format!(
                "failed to read image_analyze image `{}`: {error}",
                image.display()
            ))
        })?);

        let api_key_env = input
            .get("api_key_env")
            .filter(|value| !value.trim().is_empty())
            .unwrap_or(&self.vision.api_key_env);
        let api_key = std::env::var(api_key_env)
            .ok()
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| app_error(format!("image_analyze requires `{api_key_env}`")))?;
        let model = input
            .get("model")
            .filter(|value| !value.trim().is_empty())
            .unwrap_or(&self.vision.model);
        let base_url = input
            .get("base_url")
            .filter(|value| !value.trim().is_empty())
            .unwrap_or(&self.vision.base_url);
        let max_tokens = parse_u32_arg(&input, "max_tokens", 4096).clamp(1, 128_000);
        let endpoint = chat_completions_endpoint(base_url);
        let body = build_vision_request(model, prompt, &mime_type, &image_data, max_tokens);
        let auth = format!("Authorization: Bearer {api_key}");
        let mut child = Command::new("curl")
            .args([
                "-sS",
                "--max-time",
                "120",
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
            .map_err(|error| {
                app_error(format!("could not invoke curl for image_analyze: {error}"))
            })?;
        {
            let mut stdin = child
                .stdin
                .take()
                .ok_or_else(|| app_error("curl produced no stdin pipe for image_analyze"))?;
            stdin.write_all(body.as_bytes()).map_err(|error| {
                app_error(format!(
                    "failed to write image_analyze request body: {error}"
                ))
            })?;
        }
        let output = child.wait_with_output()?;
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        if !output.status.success() {
            return Err(app_error(format!(
                "image_analyze vision API call failed: {}",
                if stderr.is_empty() {
                    output.status.to_string()
                } else {
                    stderr
                }
            )));
        }

        let parsed = parse_image_analyze_response(&stdout, model)?;
        Ok(ToolOutput { summary: parsed })
    }
}

fn required_nonempty_any(input: &ToolInput, keys: &[&str], tool_name: &str) -> AppResult<String> {
    keys.iter()
        .find_map(|key| {
            input
                .get(key)
                .filter(|value| !value.trim().is_empty())
                .map(str::to_string)
        })
        .ok_or_else(|| app_error(format!("{tool_name} requires `{}`", keys[0])))
}

fn parse_u32_arg(input: &ToolInput, key: &str, default: u32) -> u32 {
    input
        .get(key)
        .and_then(|value| value.trim().parse::<u32>().ok())
        .unwrap_or(default)
}

fn workspace_base(input: &ToolInput) -> PathBuf {
    input
        .get("cwd")
        .or_else(|| input.get("workspace"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

fn safe_workspace_path(base: &Path, raw_path: &str, tool_name: &str) -> AppResult<PathBuf> {
    resolve_workspace_path(base, raw_path, tool_name)
}

fn detect_mime_type(path: &Path) -> AppResult<&'static str> {
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match extension.as_str() {
        "png" => Ok("image/png"),
        "jpg" | "jpeg" => Ok("image/jpeg"),
        "gif" => Ok("image/gif"),
        "webp" => Ok("image/webp"),
        "bmp" => Ok("image/bmp"),
        _ => Err(app_error(format!("Unsupported image format: {extension}"))),
    }
}

fn chat_completions_endpoint(base_url: &str) -> String {
    let base = base_url.trim_end_matches('/');
    if base.ends_with("/chat/completions") {
        base.to_string()
    } else {
        format!("{base}/chat/completions")
    }
}

fn build_vision_request(
    model: &str,
    prompt: &str,
    mime_type: &str,
    image_data: &str,
    max_tokens: u32,
) -> String {
    format!(
        concat!(
            "{{",
            "\"model\":\"{}\",",
            "\"messages\":[{{",
            "\"role\":\"user\",",
            "\"content\":[",
            "{{\"type\":\"text\",\"text\":\"{}\"}},",
            "{{\"type\":\"image_url\",\"image_url\":{{\"url\":\"data:{};base64,{}\"}}}}",
            "]",
            "}}],",
            "\"max_tokens\":{},",
            "\"temperature\":0.7",
            "}}"
        ),
        json_escape(model),
        json_escape(prompt),
        json_escape(mime_type),
        image_data,
        max_tokens
    )
}

fn parse_image_analyze_response(body: &str, fallback_model: &str) -> AppResult<String> {
    let parsed = parse_json_value(body.trim())?;
    let root = json_as_object(&parsed)
        .ok_or_else(|| app_error("image_analyze response root must be a JSON object"))?;
    if let Some(error) = root.get("error").and_then(json_as_object) {
        let message = error
            .get("message")
            .and_then(json_as_string)
            .unwrap_or("unknown vision API error");
        return Err(app_error(format!(
            "image_analyze vision API call failed: {message}"
        )));
    }
    let choices = root
        .get("choices")
        .and_then(json_as_array)
        .ok_or_else(|| app_error("image_analyze response missing `choices` array"))?;
    let first = choices
        .first()
        .and_then(json_as_object)
        .ok_or_else(|| app_error("image_analyze response missing first choice"))?;
    let analysis = first
        .get("message")
        .and_then(json_as_object)
        .and_then(|message| message.get("content"))
        .and_then(json_as_string)
        .ok_or_else(|| app_error("image_analyze response choice missing message content"))?;
    let model = root
        .get("model")
        .and_then(json_as_string)
        .unwrap_or(fallback_model);
    Ok(format!(
        "{{\"analysis\":\"{}\",\"model\":\"{}\"}}",
        json_escape(analysis),
        json_escape(model)
    ))
}

fn encode_base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut output = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0];
        let b1 = *chunk.get(1).unwrap_or(&0);
        let b2 = *chunk.get(2).unwrap_or(&0);
        output.push(TABLE[(b0 >> 2) as usize] as char);
        output.push(TABLE[(((b0 & 0b0000_0011) << 4) | (b1 >> 4)) as usize] as char);
        if chunk.len() > 1 {
            output.push(TABLE[(((b1 & 0b0000_1111) << 2) | (b2 >> 6)) as usize] as char);
        } else {
            output.push('=');
        }
        if chunk.len() > 2 {
            output.push(TABLE[(b2 & 0b0011_1111) as usize] as char);
        } else {
            output.push('=');
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root(label: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "deepseek-image-analyze-{label}-{}-{nanos}",
            std::process::id()
        ));
        fs::create_dir_all(&root).unwrap();
        root
    }

    fn test_tool(api_key_env: &str) -> ImageAnalyzeTool {
        let mut config = AppConfig::default();
        config.vision.api_key_env = api_key_env.to_string();
        ImageAnalyzeTool::new(&config)
    }

    #[test]
    fn mime_type_detection_covers_common_formats() {
        for (extension, expected) in [
            ("png", "image/png"),
            ("PNG", "image/png"),
            ("jpg", "image/jpeg"),
            ("jpeg", "image/jpeg"),
            ("gif", "image/gif"),
            ("webp", "image/webp"),
            ("bmp", "image/bmp"),
        ] {
            let path = PathBuf::from(format!("test.{extension}"));
            assert_eq!(detect_mime_type(&path).unwrap(), expected);
        }
    }

    #[test]
    fn image_analyze_rejects_absolute_path_before_api_key() {
        let tool = test_tool("DSCODE_TEST_MISSING_IMAGE_ANALYZE_KEY_ABS");
        let error = tool
            .execute(ToolInput::new().with_arg("image_path", "/etc/hosts"))
            .unwrap_err();
        assert!(error.to_string().contains("outside workspace"));
    }

    #[test]
    fn image_analyze_rejects_parent_traversal_before_api_key() {
        let tool = test_tool("DSCODE_TEST_MISSING_IMAGE_ANALYZE_KEY_PARENT");
        let error = tool
            .execute(ToolInput::new().with_arg("image_path", "../escape.png"))
            .unwrap_err();
        assert!(error.to_string().contains("outside workspace"));
    }

    #[test]
    fn image_analyze_rejects_unsupported_extension_before_api_key() {
        let root = temp_root("unsupported");
        let image = root.join("diagram.svg");
        fs::write(&image, b"<svg></svg>").unwrap();
        let tool = test_tool("DSCODE_TEST_MISSING_IMAGE_ANALYZE_KEY_UNSUPPORTED");
        let error = tool
            .execute(
                ToolInput::new()
                    .with_arg("cwd", root.display().to_string())
                    .with_arg("image_path", "diagram.svg"),
            )
            .unwrap_err();
        assert!(error.to_string().contains("Unsupported image format"));
    }

    #[test]
    fn image_analyze_missing_api_key_names_configured_env() {
        let root = temp_root("missing-key");
        let image = root.join("pixel.png");
        fs::write(&image, b"\x89PNG\r\n\x1a\n").unwrap();
        let api_key_env = "DSCODE_TEST_MISSING_IMAGE_ANALYZE_KEY_NAMED";
        std::env::remove_var(api_key_env);
        let tool = test_tool(api_key_env);
        let error = tool
            .execute(
                ToolInput::new()
                    .with_arg("cwd", root.display().to_string())
                    .with_arg("image_path", "pixel.png"),
            )
            .unwrap_err();
        assert!(error.to_string().contains(api_key_env));
    }

    #[test]
    fn image_analyze_response_parser_extracts_analysis_and_model() {
        let body = r#"{"model":"vision-live","choices":[{"message":{"content":"A chart."}}]}"#;
        let parsed = parse_image_analyze_response(body, "fallback").unwrap();
        assert_eq!(parsed, r#"{"analysis":"A chart.","model":"vision-live"}"#);
    }

    #[test]
    fn base64_encoder_matches_known_vectors() {
        assert_eq!(encode_base64(b""), "");
        assert_eq!(encode_base64(b"f"), "Zg==");
        assert_eq!(encode_base64(b"fo"), "Zm8=");
        assert_eq!(encode_base64(b"foo"), "Zm9v");
        assert_eq!(encode_base64(b"hello"), "aGVsbG8=");
    }
}
