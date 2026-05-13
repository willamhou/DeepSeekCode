use crate::error::{app_error, AppResult};
use crate::tools::types::{Tool, ToolInput, ToolOutput};
use crate::util::json::{json_as_array, json_as_object, parse_value, skip_ws, JsonValue};
use std::fs;
use std::path::Path;

pub struct ValidateDataTool;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DataFormat {
    Auto,
    Json,
    Toml,
}

impl DataFormat {
    fn parse(value: Option<&str>) -> AppResult<Self> {
        match value.unwrap_or("auto").trim().to_ascii_lowercase().as_str() {
            "auto" => Ok(Self::Auto),
            "json" => Ok(Self::Json),
            "toml" => Ok(Self::Toml),
            other => Err(app_error(format!(
                "validate_data unsupported format `{other}`; expected auto, json, or toml"
            ))),
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Json => "json",
            Self::Toml => "toml",
        }
    }
}

impl Tool for ValidateDataTool {
    fn name(&self) -> &str {
        "validate_data"
    }

    fn execute(&self, input: ToolInput) -> AppResult<ToolOutput> {
        let format = DataFormat::parse(input.get("format"))?;
        let (source, content, extension) = load_input_source(&input)?;

        let summary = match format {
            DataFormat::Json => validate_json(&content, &source),
            DataFormat::Toml => validate_toml(&content, &source),
            DataFormat::Auto => validate_auto(&content, &source, extension.as_deref()),
        };

        Ok(ToolOutput { summary })
    }
}

fn load_input_source(input: &ToolInput) -> AppResult<(String, String, Option<String>)> {
    let path = input.get("path").filter(|value| !value.trim().is_empty());
    let content = input
        .get("content")
        .filter(|value| !value.trim().is_empty());
    match (path, content) {
        (Some(_), Some(_)) => Err(app_error(
            "validate_data accepts either path or content, not both",
        )),
        (None, None) => Err(app_error("validate_data requires path or content")),
        (Some(path), None) => {
            let resolved = Path::new(path);
            let content = fs::read_to_string(resolved)?;
            let extension = resolved
                .extension()
                .and_then(|value| value.to_str())
                .map(|value| value.to_ascii_lowercase());
            Ok((path.to_string(), content, extension))
        }
        (None, Some(content)) => Ok(("inline".to_string(), content.to_string(), None)),
    }
}

fn validate_auto(content: &str, source: &str, extension: Option<&str>) -> String {
    match extension {
        Some("json") => return validate_json(content, source),
        Some("toml") => return validate_toml(content, source),
        _ => {}
    }

    match parse_json_strict(content) {
        Ok(value) => return valid_summary(DataFormat::Json, source, summarize_json(&value)),
        Err(json_error) => match validate_toml_lines(content) {
            Ok(summary) => valid_summary(DataFormat::Toml, source, summary),
            Err(toml_error) => format!(
                "valid: false\nformat: auto\nsource: {source}\njson_error: {json_error}\ntoml_error: {toml_error}"
            ),
        },
    }
}

fn validate_json(content: &str, source: &str) -> String {
    match parse_json_strict(content) {
        Ok(value) => valid_summary(DataFormat::Json, source, summarize_json(&value)),
        Err(error) => format!("valid: false\nformat: json\nsource: {source}\nerror: {error}"),
    }
}

fn validate_toml(content: &str, source: &str) -> String {
    match validate_toml_lines(content) {
        Ok(summary) => valid_summary(DataFormat::Toml, source, summary),
        Err(error) => format!("valid: false\nformat: toml\nsource: {source}\nerror: {error}"),
    }
}

fn valid_summary(format: DataFormat, source: &str, summary: String) -> String {
    format!(
        "valid: true\nformat: {}\nsource: {source}\nsummary: {summary}",
        format.as_str()
    )
}

fn parse_json_strict(content: &str) -> AppResult<JsonValue> {
    let bytes = content.as_bytes();
    let mut index = 0;
    let value = parse_value(bytes, &mut index)?;
    skip_ws(bytes, &mut index);
    if index != bytes.len() {
        return Err(app_error("trailing characters after json value"));
    }
    Ok(value)
}

fn summarize_json(value: &JsonValue) -> String {
    match value {
        JsonValue::Object(_) => {
            let Some(object) = json_as_object(value) else {
                return "top_level=object".to_string();
            };
            let keys = object
                .keys()
                .take(10)
                .cloned()
                .collect::<Vec<_>>()
                .join(",");
            format!(
                "top_level=object entries={} keys_preview={keys}",
                object.len()
            )
        }
        JsonValue::Array(_) => {
            let len = json_as_array(value).map(Vec::len).unwrap_or(0);
            format!("top_level=array entries={len}")
        }
        JsonValue::String(_) => "top_level=string".to_string(),
        JsonValue::Number(_) => "top_level=number".to_string(),
        JsonValue::Bool(_) => "top_level=boolean".to_string(),
        JsonValue::Null => "top_level=null".to_string(),
    }
}

fn validate_toml_lines(content: &str) -> Result<String, String> {
    let mut entries = 0usize;
    let mut sections = 0usize;
    let mut keys = Vec::new();

    for (index, raw_line) in content.lines().enumerate() {
        let line_number = index + 1;
        let line = strip_toml_comment(raw_line).trim();
        if line.is_empty() {
            continue;
        }

        if let Some(section) = line
            .strip_prefix("[[")
            .and_then(|value| value.strip_suffix("]]"))
        {
            validate_toml_key_path(section.trim())
                .map_err(|error| format!("line {line_number}: {error}"))?;
            sections += 1;
            continue;
        }
        if let Some(section) = line
            .strip_prefix('[')
            .and_then(|value| value.strip_suffix(']'))
        {
            validate_toml_key_path(section.trim())
                .map_err(|error| format!("line {line_number}: {error}"))?;
            sections += 1;
            continue;
        }

        let Some((key, value)) = line.split_once('=') else {
            return Err(format!(
                "line {line_number}: expected key = value or section header"
            ));
        };
        validate_toml_key_path(key.trim())
            .map_err(|error| format!("line {line_number}: {error}"))?;
        validate_toml_value(value.trim())
            .map_err(|error| format!("line {line_number}: {error}"))?;
        entries += 1;
        if keys.len() < 10 {
            keys.push(key.trim().to_string());
        }
    }

    Ok(format!(
        "top_level=table entries={entries} sections={sections} keys_preview={}",
        keys.join(",")
    ))
}

fn strip_toml_comment(line: &str) -> &str {
    let mut in_single = false;
    let mut in_double = false;
    let mut escaped = false;
    for (index, ch) in line.char_indices() {
        match ch {
            '\\' if in_double => escaped = !escaped,
            '"' if !in_single && !escaped => in_double = !in_double,
            '\'' if !in_double => in_single = !in_single,
            '#' if !in_single && !in_double => return &line[..index],
            _ => escaped = false,
        }
    }
    line
}

fn validate_toml_key_path(value: &str) -> Result<(), String> {
    if value.is_empty() {
        return Err("empty TOML key or section".to_string());
    }
    for segment in value.split('.') {
        let segment = segment.trim();
        if segment.is_empty() {
            return Err("empty TOML dotted key segment".to_string());
        }
        if !(is_quoted_toml_string(segment)
            || segment
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-'))
        {
            return Err(format!("invalid TOML key segment `{segment}`"));
        }
    }
    Ok(())
}

fn validate_toml_value(value: &str) -> Result<(), String> {
    if value.is_empty() {
        return Err("empty TOML value".to_string());
    }
    if is_quoted_toml_string(value)
        || matches!(value, "true" | "false")
        || is_toml_number(value)
        || is_toml_datetime_like(value)
        || is_balanced_toml_container(value, '[', ']')
        || is_balanced_toml_container(value, '{', '}')
    {
        Ok(())
    } else {
        Err(format!("unsupported TOML value `{value}`"))
    }
}

fn is_quoted_toml_string(value: &str) -> bool {
    (value.len() >= 2 && value.starts_with('"') && value.ends_with('"'))
        || (value.len() >= 2 && value.starts_with('\'') && value.ends_with('\''))
}

fn is_toml_number(value: &str) -> bool {
    let cleaned = value.replace('_', "");
    cleaned.parse::<i64>().is_ok() || cleaned.parse::<f64>().is_ok()
}

fn is_toml_datetime_like(value: &str) -> bool {
    value.contains('-')
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | ':' | '.' | '+' | 'T' | 'Z'))
}

fn is_balanced_toml_container(value: &str, open: char, close: char) -> bool {
    if !value.starts_with(open) || !value.ends_with(close) {
        return false;
    }
    let mut depth = 0isize;
    let mut in_single = false;
    let mut in_double = false;
    let mut escaped = false;
    for ch in value.chars() {
        match ch {
            '\\' if in_double => escaped = !escaped,
            '"' if !in_single && !escaped => in_double = !in_double,
            '\'' if !in_double => in_single = !in_single,
            ch if ch == open && !in_single && !in_double => depth += 1,
            ch if ch == close && !in_single && !in_double => depth -= 1,
            _ => escaped = false,
        }
        if depth < 0 {
            return false;
        }
    }
    depth == 0 && !in_single && !in_double
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
            "deepseek-validate-data-{name}-{}-{suffix}",
            std::process::id()
        ))
    }

    #[test]
    fn validate_data_json_content_succeeds() {
        let output = ValidateDataTool
            .execute(
                ToolInput::new()
                    .with_arg("content", r#"{"name":"deepseek","items":[1,2]}"#)
                    .with_arg("format", "json"),
            )
            .unwrap();

        assert!(output.summary.contains("valid: true"));
        assert!(output.summary.contains("format: json"));
        assert!(output.summary.contains("top_level=object"));
    }

    #[test]
    fn validate_data_toml_file_succeeds() {
        let root = temp_root("toml");
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("config.toml");
        std::fs::write(&path, "name = \"deepseek\"\n[model]\nmax_tokens = 1000\n").unwrap();

        let output = ValidateDataTool
            .execute(
                ToolInput::new()
                    .with_arg("path", path.display().to_string())
                    .with_arg("format", "auto"),
            )
            .unwrap();

        assert!(output.summary.contains("valid: true"));
        assert!(output.summary.contains("format: toml"));
        assert!(output.summary.contains("sections=1"));
    }

    #[test]
    fn validate_data_auto_reports_invalid_content() {
        let output = ValidateDataTool
            .execute(ToolInput::new().with_arg("content", "not-valid-data"))
            .unwrap();

        assert!(output.summary.contains("valid: false"));
        assert!(output.summary.contains("json_error:"));
        assert!(output.summary.contains("toml_error:"));
    }

    #[test]
    fn validate_data_rejects_path_and_content_together() {
        let error = ValidateDataTool
            .execute(
                ToolInput::new()
                    .with_arg("path", "config.toml")
                    .with_arg("content", "name = \"deepseek\""),
            )
            .unwrap_err();

        assert!(error.to_string().contains("either path or content"));
    }
}
