use std::collections::{BTreeMap, BTreeSet};

use crate::error::{app_error, AppResult};
use crate::model::protocol::ToolCallRequest;
use crate::tools::types::ToolInput;
use crate::util::json::{
    json_as_array, json_as_object, json_as_string, json_value_to_string, parse_value, JsonValue,
};

const MAX_SCAVENGE_BYTES: usize = 100_000;
const MAX_SCAVENGED_CALLS: usize = 4;
const MAX_REPAIRED_ARGUMENT_BYTES: usize = 32_768;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolRepairNote {
    pub kind: &'static str,
    pub detail: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ToolSchemaTransform {
    flat_paths: BTreeMap<String, Vec<String>>,
}

impl ToolSchemaTransform {
    pub fn is_empty(&self) -> bool {
        self.flat_paths.is_empty()
    }
}

struct FlattenLeaf {
    flat_key: String,
    path: Vec<String>,
    schema: JsonValue,
    required: bool,
}

impl ToolRepairNote {
    fn new(kind: &'static str, detail: impl Into<String>) -> Self {
        Self {
            kind,
            detail: detail.into(),
        }
    }
}

pub fn flatten_tool_schema_auto(
    properties_json: &str,
    required_json: &str,
) -> Option<(String, String, ToolSchemaTransform)> {
    let properties = parse_root_object_complete(properties_json).ok()?;
    let root_required = parse_json_string_array(required_json).unwrap_or_default();
    let mut leaves = Vec::new();
    let mut max_path_len = 0_usize;
    for (name, schema) in properties {
        let path = vec![name.clone()];
        let required = root_required.contains(&name);
        collect_flatten_leaves(path, schema, required, &mut leaves, &mut max_path_len);
    }

    if max_path_len <= 1 && leaves.len() <= 10 {
        return None;
    }

    let mut flat_properties = BTreeMap::new();
    let mut required = Vec::new();
    let mut transform = ToolSchemaTransform::default();
    for leaf in leaves {
        if flat_properties.contains_key(&leaf.flat_key) {
            return None;
        }
        if leaf.path.len() > 1 {
            transform
                .flat_paths
                .insert(leaf.flat_key.clone(), leaf.path.clone());
        }
        if leaf.required {
            required.push(JsonValue::String(leaf.flat_key.clone()));
        }
        flat_properties.insert(leaf.flat_key, leaf.schema);
    }

    if transform.is_empty() {
        return None;
    }

    Some((
        json_value_to_string(&JsonValue::Object(flat_properties)),
        json_value_to_string(&JsonValue::Array(required)),
        transform,
    ))
}

pub fn re_nest_tool_arguments(
    args: BTreeMap<String, String>,
    transform: &ToolSchemaTransform,
) -> BTreeMap<String, String> {
    if transform.is_empty() {
        return args;
    }

    let original = args.clone();
    let mut root = BTreeMap::new();
    for (key, value) in args {
        if let Some(path) = transform.flat_paths.get(&key) {
            insert_path_value(&mut root, path, value);
        } else {
            root.insert(key, JsonValue::String(value));
        }
    }
    json_object_to_string_args(&JsonValue::Object(root)).unwrap_or(original)
}

pub fn parse_tool_arguments_with_repair(
    raw: &str,
) -> AppResult<(BTreeMap<String, String>, Option<ToolRepairNote>)> {
    match parse_tool_arguments_strict(raw) {
        Ok(args) => Ok((args, None)),
        Err(original) => {
            let Some(repaired) = repair_truncated_json(raw) else {
                return Err(original);
            };
            let args = parse_tool_arguments_strict(&repaired)?;
            Ok((
                args,
                Some(ToolRepairNote::new(
                    "truncated-json",
                    "repaired truncated tool arguments JSON",
                )),
            ))
        }
    }
}

pub fn scavenge_tool_calls<S: AsRef<str>>(
    text: &str,
    known_tools: &[S],
) -> (Vec<ToolCallRequest>, Vec<ToolRepairNote>) {
    if text.trim().is_empty() || known_tools.is_empty() {
        return (Vec::new(), Vec::new());
    }
    let known = known_tools
        .iter()
        .map(|tool| tool.as_ref())
        .collect::<BTreeSet<_>>();
    let haystack = truncate_for_scavenge(text);
    let mut calls = Vec::new();
    let mut notes = Vec::new();

    for span in json_object_spans(haystack) {
        if calls.len() >= MAX_SCAVENGED_CALLS {
            break;
        }
        let Ok(root) = parse_root_object_complete(span) else {
            continue;
        };
        let Some(call) = call_from_root(&root, &known) else {
            continue;
        };
        notes.push(ToolRepairNote::new(
            "scavenged-tool-call",
            format!("scavenged `{}` from assistant text", call.tool_name),
        ));
        calls.push(call);
    }

    (calls, notes)
}

pub fn repair_truncated_json(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty()
        || trimmed.len() > MAX_REPAIRED_ARGUMENT_BYTES
        || !trimmed.starts_with('{')
    {
        return None;
    }
    if parse_root_object_complete(trimmed).is_ok() {
        return Some(trimmed.to_string());
    }

    let mut stack = Vec::new();
    let mut in_string = false;
    let mut escaped = false;
    for byte in trimmed.bytes() {
        if in_string {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                in_string = false;
            }
            continue;
        }

        match byte {
            b'"' => in_string = true,
            b'{' => stack.push(b'}'),
            b'[' => stack.push(b']'),
            b'}' | b']' => {
                if stack.pop() != Some(byte) {
                    return None;
                }
            }
            _ => {}
        }
    }

    let mut out = trimmed.to_string();
    if in_string {
        out.push('"');
    }
    trim_trailing_comma(&mut out);
    if ends_with_colon(&out) {
        out.push_str("null");
    }
    for closer in stack.into_iter().rev() {
        out.push(closer as char);
    }

    parse_root_object_complete(&out).ok()?;
    Some(out)
}

fn parse_tool_arguments_strict(raw: &str) -> AppResult<BTreeMap<String, String>> {
    let root = parse_root_object_complete(raw)?;
    json_object_to_string_args(&JsonValue::Object(root))
}

fn parse_root_object_complete(input: &str) -> AppResult<BTreeMap<String, JsonValue>> {
    let trimmed = input.trim();
    let bytes = trimmed.as_bytes();
    let mut index = 0;
    let value = parse_value(bytes, &mut index)?;
    if bytes[index..]
        .iter()
        .any(|byte| !byte.is_ascii_whitespace())
    {
        return Err(app_error("unexpected trailing json input"));
    }
    let JsonValue::Object(root) = value else {
        return Err(app_error("json root must be an object"));
    };
    Ok(root)
}

fn parse_json_string_array(input: &str) -> AppResult<BTreeSet<String>> {
    let bytes = input.trim().as_bytes();
    let mut index = 0;
    let value = parse_value(bytes, &mut index)?;
    if bytes[index..]
        .iter()
        .any(|byte| !byte.is_ascii_whitespace())
    {
        return Err(app_error("unexpected trailing json input"));
    }
    let JsonValue::Array(items) = value else {
        return Err(app_error("json root must be an array"));
    };
    Ok(items
        .into_iter()
        .filter_map(|value| match value {
            JsonValue::String(value) => Some(value),
            _ => None,
        })
        .collect())
}

fn collect_flatten_leaves(
    path: Vec<String>,
    schema: JsonValue,
    required: bool,
    leaves: &mut Vec<FlattenLeaf>,
    max_path_len: &mut usize,
) {
    let Some(schema_object) = json_as_object(&schema) else {
        push_flatten_leaf(path, schema, required, leaves, max_path_len);
        return;
    };
    let is_object_schema = schema_object.get("type").and_then(json_as_string) == Some("object");
    let Some(properties) = schema_object.get("properties").and_then(json_as_object) else {
        push_flatten_leaf(path, schema, required, leaves, max_path_len);
        return;
    };
    if !is_object_schema || properties.is_empty() {
        push_flatten_leaf(path, schema, required, leaves, max_path_len);
        return;
    }

    let nested_required = schema_object
        .get("required")
        .and_then(json_as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(json_as_string)
                .map(str::to_string)
                .collect::<BTreeSet<_>>()
        })
        .unwrap_or_default();
    for (name, child_schema) in properties {
        let mut child_path = path.clone();
        child_path.push(name.clone());
        collect_flatten_leaves(
            child_path,
            child_schema.clone(),
            required && nested_required.contains(name),
            leaves,
            max_path_len,
        );
    }
}

fn push_flatten_leaf(
    path: Vec<String>,
    schema: JsonValue,
    required: bool,
    leaves: &mut Vec<FlattenLeaf>,
    max_path_len: &mut usize,
) {
    *max_path_len = (*max_path_len).max(path.len());
    leaves.push(FlattenLeaf {
        flat_key: path.join("."),
        path,
        schema,
        required,
    });
}

fn insert_path_value(root: &mut BTreeMap<String, JsonValue>, path: &[String], value: String) {
    let Some((head, tail)) = path.split_first() else {
        return;
    };
    if tail.is_empty() {
        root.insert(head.clone(), JsonValue::String(value));
        return;
    }
    let entry = root
        .entry(head.clone())
        .or_insert_with(|| JsonValue::Object(BTreeMap::new()));
    if !matches!(entry, JsonValue::Object(_)) {
        *entry = JsonValue::Object(BTreeMap::new());
    }
    let JsonValue::Object(child) = entry else {
        return;
    };
    insert_path_value(child, tail, value);
}

fn json_object_to_string_args(value: &JsonValue) -> AppResult<BTreeMap<String, String>> {
    let JsonValue::Object(map) = value else {
        return Err(app_error("tool input must be a json object"));
    };

    let mut result = BTreeMap::new();
    for (key, value) in map {
        match value {
            JsonValue::String(value) => {
                result.insert(key.clone(), value.clone());
            }
            JsonValue::Number(value) => {
                result.insert(key.clone(), value.clone());
            }
            JsonValue::Bool(value) => {
                result.insert(
                    key.clone(),
                    if *value { "true" } else { "false" }.to_string(),
                );
            }
            JsonValue::Null => {
                result.insert(key.clone(), "null".to_string());
            }
            JsonValue::Object(_) | JsonValue::Array(_) => {
                result.insert(key.clone(), json_value_to_string(value));
            }
        }
    }
    Ok(result)
}

fn truncate_for_scavenge(text: &str) -> &str {
    if text.len() <= MAX_SCAVENGE_BYTES {
        return text;
    }
    let mut end = MAX_SCAVENGE_BYTES;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

fn json_object_spans(text: &str) -> Vec<&str> {
    let bytes = text.as_bytes();
    let mut spans = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'{' {
            i += 1;
            continue;
        }
        if let Some(end) = balanced_object_end(bytes, i) {
            if let Some(span) = text.get(i..end) {
                spans.push(span);
            }
            i = end;
        } else {
            i += 1;
        }
    }
    spans
}

fn balanced_object_end(bytes: &[u8], start: usize) -> Option<usize> {
    let mut stack = Vec::new();
    let mut in_string = false;
    let mut escaped = false;
    let mut i = start;
    while i < bytes.len() {
        let byte = bytes[i];
        if in_string {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                in_string = false;
            }
            i += 1;
            continue;
        }

        match byte {
            b'"' => in_string = true,
            b'{' => stack.push(b'}'),
            b'[' => stack.push(b']'),
            b'}' | b']' => {
                if stack.pop() != Some(byte) {
                    return None;
                }
                if stack.is_empty() {
                    return Some(i + 1);
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

fn call_from_root(
    root: &BTreeMap<String, JsonValue>,
    known: &BTreeSet<&str>,
) -> Option<ToolCallRequest> {
    if let Some(function) = root.get("function").and_then(json_as_object) {
        return call_from_name_and_args(
            function.get("name").and_then(json_as_string),
            function.get("arguments"),
            known,
        );
    }

    if let Some(tool_call) = root.get("tool_call").and_then(json_as_object) {
        if let Some(call) = call_from_root(tool_call, known) {
            return Some(call);
        }
    }

    let name = root
        .get("tool_name")
        .or_else(|| root.get("name"))
        .or_else(|| root.get("tool"))
        .and_then(json_as_string);
    let args = root
        .get("arguments")
        .or_else(|| root.get("args"))
        .or_else(|| root.get("input"));
    call_from_name_and_args(name, args, known)
}

fn call_from_name_and_args(
    name: Option<&str>,
    args: Option<&JsonValue>,
    known: &BTreeSet<&str>,
) -> Option<ToolCallRequest> {
    let name = name?;
    if !known.contains(name) {
        return None;
    }
    let input = match args {
        None | Some(JsonValue::Null) => BTreeMap::new(),
        Some(JsonValue::String(raw)) => parse_tool_arguments_with_repair(raw).ok()?.0,
        Some(JsonValue::Object(_)) => json_object_to_string_args(args?).ok()?,
        Some(_) => return None,
    };
    Some(ToolCallRequest {
        tool_name: name.to_string(),
        input: ToolInput { args: input },
    })
}

fn trim_trailing_comma(out: &mut String) {
    loop {
        let trimmed_len = out.trim_end().len();
        out.truncate(trimmed_len);
        if !out.ends_with(',') {
            break;
        }
        out.pop();
    }
}

fn ends_with_colon(out: &str) -> bool {
    out.trim_end().ends_with(':')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repairs_truncated_object_value_string() {
        let repaired = repair_truncated_json(r#"{"path":"src/main.rs""#).unwrap();
        assert_eq!(repaired, r#"{"path":"src/main.rs"}"#);
        let (args, note) = parse_tool_arguments_with_repair(r#"{"path":"src/main.rs""#).unwrap();
        assert_eq!(args.get("path").map(String::as_str), Some("src/main.rs"));
        assert_eq!(note.unwrap().kind, "truncated-json");
    }

    #[test]
    fn repairs_trailing_comma_and_nested_array() {
        let raw = r#"{"items":[{"title":"one"}],"#;
        let repaired = repair_truncated_json(raw).unwrap();
        assert_eq!(repaired, r#"{"items":[{"title":"one"}]}"#);
        let (args, _) = parse_tool_arguments_with_repair(raw).unwrap();
        assert_eq!(
            args.get("items").map(String::as_str),
            Some(r#"[{"title":"one"}]"#)
        );
    }

    #[test]
    fn repairs_dangling_colon_with_null() {
        let (args, _) = parse_tool_arguments_with_repair(r#"{"path":"a.rs","limit":"#).unwrap();
        assert_eq!(args.get("path").map(String::as_str), Some("a.rs"));
        assert_eq!(args.get("limit").map(String::as_str), Some("null"));
    }

    #[test]
    fn rejects_non_json_garbage() {
        assert!(repair_truncated_json("NOT_JSON").is_none());
        assert!(parse_tool_arguments_with_repair("{not_json").is_err());
    }

    #[test]
    fn rejects_trailing_garbage_after_object() {
        assert!(parse_tool_arguments_with_repair(r#"{"path":"src/lib.rs"} trailing"#).is_err());
        assert!(repair_truncated_json(r#"{"path":"src/lib.rs"} trailing"#).is_none());
    }

    #[test]
    fn scavenges_known_tool_call_from_text() {
        let text = r#"thinking...
{"tool_name":"read_file","arguments":{"path":"src/lib.rs"}}
done"#;
        let (calls, notes) = scavenge_tool_calls(text, &["read_file", "git_diff"]);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].tool_name, "read_file");
        assert_eq!(
            calls[0].input.args.get("path").map(String::as_str),
            Some("src/lib.rs")
        );
        assert_eq!(notes[0].kind, "scavenged-tool-call");
    }

    #[test]
    fn scavenges_openai_shaped_function_call() {
        let text = r#"{"function":{"name":"git_diff","arguments":"{}"}}"#;
        let (calls, notes) = scavenge_tool_calls(text, &["git_diff"]);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].tool_name, "git_diff");
        assert!(calls[0].input.args.is_empty());
        assert_eq!(notes.len(), 1);
    }

    #[test]
    fn scavenge_rejects_unknown_tool() {
        let text = r#"{"tool_name":"delete_everything","arguments":{"path":"."}}"#;
        let (calls, notes) = scavenge_tool_calls(text, &["read_file"]);
        assert!(calls.is_empty());
        assert!(notes.is_empty());
    }

    #[test]
    fn flattens_nested_object_schema_and_re_nests_arguments() {
        let properties = r#"{"target":{"type":"object","description":"Target file","properties":{"path":{"type":"string","description":"Path"},"range":{"type":"object","properties":{"start":{"type":"string"},"end":{"type":"string"}},"required":["start"]}},"required":["path","range"]},"dry_run":{"type":"string"}}"#;
        let required = r#"["target"]"#;
        let (flat_properties, flat_required, transform) =
            flatten_tool_schema_auto(properties, required).expect("schema should flatten");

        assert!(flat_properties.contains("\"target.path\""));
        assert!(flat_properties.contains("\"target.range.start\""));
        assert!(flat_required.contains("\"target.path\""));
        assert!(flat_required.contains("\"target.range.start\""));
        assert!(!flat_required.contains("\"target.range.end\""));

        let mut args = BTreeMap::new();
        args.insert("target.path".to_string(), "src/lib.rs".to_string());
        args.insert("target.range.start".to_string(), "10".to_string());
        args.insert("dry_run".to_string(), "true".to_string());
        let nested = re_nest_tool_arguments(args, &transform);
        assert_eq!(nested.get("dry_run").map(String::as_str), Some("true"));
        assert_eq!(
            nested.get("target").map(String::as_str),
            Some(r#"{"path":"src/lib.rs","range":{"start":"10"}}"#)
        );
    }

    #[test]
    fn shallow_schema_does_not_flatten() {
        let properties = r#"{"path":{"type":"string"},"limit":{"type":"string"}}"#;
        assert!(flatten_tool_schema_auto(properties, r#"["path"]"#).is_none());
    }
}
