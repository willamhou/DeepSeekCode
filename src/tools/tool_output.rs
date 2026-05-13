use crate::error::{app_error, AppResult};
use crate::tools::types::{Tool, ToolInput, ToolOutput};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

const SPILLOVER_DIR_NAME: &str = "tool_outputs";
const SPILLOVER_THRESHOLD_BYTES: usize = 100 * 1024;
const SPILLOVER_HEAD_BYTES: usize = 32 * 1024;
const DEFAULT_MAX_BYTES: usize = 8 * 1024;
const HARD_MAX_BYTES: usize = 128 * 1024;
const DEFAULT_LINE_COUNT: usize = 40;
const HARD_LINE_COUNT: usize = 500;
const DEFAULT_MAX_MATCHES: usize = 20;
const HARD_MAX_MATCHES: usize = 100;
const DEFAULT_CONTEXT_LINES: usize = 1;
const HARD_CONTEXT_LINES: usize = 5;

static SPILLOVER_COUNTER: AtomicU64 = AtomicU64::new(0);

pub struct RetrieveToolResultTool;

impl Tool for RetrieveToolResultTool {
    fn name(&self) -> &str {
        "retrieve_tool_result"
    }

    fn execute(&self, input: ToolInput) -> AppResult<ToolOutput> {
        let reference = input
            .get("ref")
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| app_error("retrieve_tool_result requires ref"))?;
        let mode = input
            .get("mode")
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("summary")
            .to_ascii_lowercase();
        let max_bytes =
            input_usize(&input, "max_bytes", DEFAULT_MAX_BYTES).clamp(1, HARD_MAX_BYTES);
        let path = resolve_spillover_reference(reference)?;
        let content = fs::read_to_string(&path)?;
        let lines = content.lines().collect::<Vec<_>>();

        let summary = match mode.as_str() {
            "summary" => render_summary(reference, &path, &content, &lines, &input, max_bytes),
            "head" => render_head_tail(reference, &path, "head", &lines, &input, max_bytes),
            "tail" => render_head_tail(reference, &path, "tail", &lines, &input, max_bytes),
            "lines" => render_lines(reference, &path, &lines, &input, max_bytes)?,
            "query" => render_query(reference, &path, &lines, &input, max_bytes)?,
            other => {
                return Err(app_error(format!(
                    "unsupported retrieve_tool_result mode `{other}` (expected summary, head, tail, lines, or query)"
                )));
            }
        };
        Ok(ToolOutput { summary })
    }
}

pub fn maybe_spill_successful_tool_output(tool_name: &str, summary: &str) -> String {
    if summary.len() <= SPILLOVER_THRESHOLD_BYTES {
        return summary.to_string();
    }
    let Some(root) = spillover_root() else {
        return summary.to_string();
    };
    let id = generated_spillover_id(tool_name);
    let path = root.join(format!("{id}.txt"));
    let write_result = fs::create_dir_all(&root).and_then(|_| fs::write(&path, summary));
    if write_result.is_err() {
        return summary.to_string();
    }

    let head = truncate_to_char_boundary(summary, SPILLOVER_HEAD_BYTES);
    format!(
        "{head}\n\n[large tool output spilled]\n\
         tool_output_ref: {id}\n\
         full_output_path: {}\n\
         original_bytes: {}\n\
         inline_head_bytes: {}\n\
         retrieve: retrieve_tool_result ref={id} mode=tail\n\
         retrieve_query: retrieve_tool_result ref={id} mode=query query=<text>",
        path.display(),
        summary.len(),
        head.len()
    )
}

fn render_summary(
    reference: &str,
    path: &Path,
    content: &str,
    lines: &[&str],
    input: &ToolInput,
    max_bytes: usize,
) -> String {
    let max_matches =
        input_usize(input, "max_matches", DEFAULT_MAX_MATCHES).clamp(1, HARD_MAX_MATCHES);
    let signal_lines = collect_signal_lines(lines, max_matches)
        .into_iter()
        .map(|(line_no, text)| format!("{line_no}: {}", truncate_chars(text.trim(), 300)))
        .collect::<Vec<_>>();
    let head_count = DEFAULT_LINE_COUNT.min(lines.len());
    let tail_count = DEFAULT_LINE_COUNT.min(lines.len());
    let tail_start = lines.len().saturating_sub(tail_count);
    let head = render_numbered_lines(
        lines
            .iter()
            .take(head_count)
            .enumerate()
            .map(|(index, line)| (index + 1, *line)),
        max_bytes / 2,
    );
    let tail = render_numbered_lines(
        lines
            .iter()
            .enumerate()
            .skip(tail_start)
            .map(|(index, line)| (index + 1, *line)),
        max_bytes / 2,
    );
    let mut out = format!(
        "ref: {reference}\npath: {}\nmode: summary\ntotal_bytes: {}\ntotal_lines: {}\nnon_empty_lines: {}\n",
        path.display(),
        content.len(),
        lines.len(),
        lines.iter().filter(|line| !line.trim().is_empty()).count()
    );
    if !signal_lines.is_empty() {
        out.push_str("signal_lines:\n");
        out.push_str(&signal_lines.join("\n"));
        out.push('\n');
    }
    out.push_str("head:\n");
    out.push_str(&head);
    out.push_str("\n\ntail:\n");
    out.push_str(&tail);
    out.push_str("\nhint: Use mode=head, tail, lines, or query to retrieve a narrower slice.");
    out
}

fn render_head_tail(
    reference: &str,
    path: &Path,
    mode: &str,
    lines: &[&str],
    input: &ToolInput,
    max_bytes: usize,
) -> String {
    let count = input_usize(input, "line_count", DEFAULT_LINE_COUNT).clamp(1, HARD_LINE_COUNT);
    let selected = if mode == "head" {
        lines
            .iter()
            .take(count)
            .enumerate()
            .map(|(index, line)| (index + 1, *line))
            .collect::<Vec<_>>()
    } else {
        let start = lines.len().saturating_sub(count);
        lines
            .iter()
            .enumerate()
            .skip(start)
            .map(|(index, line)| (index + 1, *line))
            .collect::<Vec<_>>()
    };
    let excerpt = render_numbered_lines(selected, max_bytes);
    format!(
        "ref: {reference}\npath: {}\nmode: {mode}\ntotal_lines: {}\nline_count: {count}\nexcerpt:\n{excerpt}",
        path.display(),
        lines.len()
    )
}

fn render_lines(
    reference: &str,
    path: &Path,
    lines: &[&str],
    input: &ToolInput,
    max_bytes: usize,
) -> AppResult<String> {
    let (start, end) = parse_line_selector(input)?;
    let capped_end = end.min(lines.len());
    let excerpt = if start > lines.len() {
        String::new()
    } else {
        render_numbered_lines(
            lines
                .iter()
                .enumerate()
                .skip(start - 1)
                .take(capped_end.saturating_sub(start) + 1)
                .map(|(index, line)| (index + 1, *line)),
            max_bytes,
        )
    };
    Ok(format!(
        "ref: {reference}\npath: {}\nmode: lines\ntotal_lines: {}\nstart_line: {start}\nend_line: {capped_end}\nexcerpt:\n{excerpt}",
        path.display(),
        lines.len()
    ))
}

fn render_query(
    reference: &str,
    path: &Path,
    lines: &[&str],
    input: &ToolInput,
    max_bytes: usize,
) -> AppResult<String> {
    let query = input
        .get("query")
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| app_error("query is required when mode=query"))?;
    let query_lower = query.to_lowercase();
    let max_matches =
        input_usize(input, "max_matches", DEFAULT_MAX_MATCHES).clamp(1, HARD_MAX_MATCHES);
    let context_lines =
        input_usize(input, "context_lines", DEFAULT_CONTEXT_LINES).clamp(0, HARD_CONTEXT_LINES);

    let mut matched_lines = 0usize;
    let mut rendered = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        if !line.to_lowercase().contains(&query_lower) {
            continue;
        }
        matched_lines += 1;
        if rendered.len() >= max_matches {
            continue;
        }
        let start = index.saturating_sub(context_lines);
        let end = (index + context_lines).min(lines.len().saturating_sub(1));
        let excerpt = render_numbered_lines(
            lines
                .iter()
                .enumerate()
                .skip(start)
                .take(end.saturating_sub(start) + 1)
                .map(|(line_index, text)| (line_index + 1, *text)),
            max_bytes / max_matches.max(1),
        );
        rendered.push(format!("match_line: {}\n{}", index + 1, excerpt));
    }

    Ok(format!(
        "ref: {reference}\npath: {}\nmode: query\nquery: {query}\ntotal_lines: {}\nmatched_lines: {matched_lines}\nmatches_returned: {}\nresults:\n{}",
        path.display(),
        lines.len(),
        rendered.len(),
        rendered.join("\n---\n")
    ))
}

fn parse_line_selector(input: &ToolInput) -> AppResult<(usize, usize)> {
    let explicit_start = input.get("start_line").and_then(|value| value.parse().ok());
    let explicit_end = input.get("end_line").and_then(|value| value.parse().ok());
    if explicit_start.is_some() || explicit_end.is_some() {
        let start = explicit_start
            .ok_or_else(|| app_error("start_line is required when end_line is supplied"))?;
        let end = explicit_end.unwrap_or(start);
        return validate_line_range(start, end);
    }
    let selector = input
        .get("lines")
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            app_error("mode=lines requires `lines` such as `10-40` or start_line/end_line")
        })?;
    if let Some((start, end)) = selector.split_once('-') {
        validate_line_range(
            parse_line_number(start, "lines start")?,
            parse_line_number(end, "lines end")?,
        )
    } else {
        let line = parse_line_number(selector, "lines")?;
        validate_line_range(line, line)
    }
}

fn validate_line_range(start: usize, end: usize) -> AppResult<(usize, usize)> {
    if start == 0 || end == 0 {
        return Err(app_error("line numbers are 1-based"));
    }
    if end < start {
        return Err(app_error(
            "end_line must be greater than or equal to start_line",
        ));
    }
    Ok((start, end))
}

fn parse_line_number(raw: &str, field: &str) -> AppResult<usize> {
    raw.trim()
        .parse::<usize>()
        .map_err(|_| app_error(format!("{field} must be a positive integer line number")))
}

fn resolve_spillover_reference(reference: &str) -> AppResult<PathBuf> {
    let root = spillover_root().ok_or_else(|| app_error("could not resolve spillover root"))?;
    let root_canonical = root.canonicalize().map_err(|error| {
        app_error(format!(
            "spillover directory {} is not readable: {error}",
            root.display()
        ))
    })?;
    let stripped = reference
        .trim()
        .strip_prefix("tool_result:")
        .unwrap_or_else(|| reference.trim());
    let raw_path = PathBuf::from(stripped);
    let candidate = if raw_path.is_absolute() {
        raw_path
    } else if stripped.ends_with(".txt") || stripped.contains('/') || stripped.contains('\\') {
        root.join(stripped)
    } else {
        spillover_path_for_id(stripped)?
    };
    let canonical = candidate.canonicalize().map_err(|error| {
        app_error(format!(
            "spilled tool result `{reference}` was not found at {}: {error}",
            candidate.display()
        ))
    })?;
    if !canonical.starts_with(&root_canonical) {
        return Err(app_error(format!(
            "ref `{reference}` does not point inside {}",
            root_canonical.display()
        )));
    }
    if !canonical.is_file() {
        return Err(app_error(format!(
            "ref `{reference}` does not point to a spillover file"
        )));
    }
    Ok(canonical)
}

fn spillover_path_for_id(id: &str) -> AppResult<PathBuf> {
    let sanitized = sanitize_id(id)
        .ok_or_else(|| app_error(format!("invalid spilled tool-result ref `{id}`")))?;
    spillover_root()
        .map(|root| root.join(format!("{sanitized}.txt")))
        .ok_or_else(|| app_error("could not resolve spillover root"))
}

fn spillover_root() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("DSCODE_TOOL_OUTPUT_ROOT") {
        if !path.trim().is_empty() {
            return Some(PathBuf::from(path));
        }
    }
    let home = std::env::var_os("HOME")?;
    Some(
        PathBuf::from(home)
            .join(".deepseek")
            .join(SPILLOVER_DIR_NAME),
    )
}

fn generated_spillover_id(tool_name: &str) -> String {
    let sanitized_tool = sanitize_id(tool_name).unwrap_or_else(|| "tool".to_string());
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    let counter = SPILLOVER_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{sanitized_tool}-{}-{nanos}-{counter}", std::process::id())
}

fn sanitize_id(id: &str) -> Option<String> {
    let sanitized = id
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || *ch == '-' || *ch == '_')
        .collect::<String>();
    if sanitized.is_empty() {
        None
    } else {
        Some(sanitized)
    }
}

fn collect_signal_lines<'a>(lines: &'a [&str], max_matches: usize) -> Vec<(usize, &'a str)> {
    let mut out = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        if is_signal_line(line) {
            out.push((index + 1, *line));
            if out.len() >= max_matches {
                break;
            }
        }
    }
    out
}

fn is_signal_line(line: &str) -> bool {
    let lower = line.to_lowercase();
    [
        "error",
        "failed",
        "failure",
        "panic",
        "warning",
        "exception",
        "traceback",
        "assertion",
        "exit code",
        "test result",
        "thread '",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
}

fn render_numbered_lines<'a>(
    lines: impl IntoIterator<Item = (usize, &'a str)>,
    max_bytes: usize,
) -> String {
    let mut rendered = String::new();
    for (line_no, line) in lines {
        rendered.push_str(&format!("{line_no}: {line}\n"));
        if rendered.len() > max_bytes {
            break;
        }
    }
    truncate_to_char_boundary(rendered.trim_end_matches('\n'), max_bytes).to_string()
}

fn input_usize(input: &ToolInput, key: &str, default: usize) -> usize {
    input
        .get(key)
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(default)
}

fn truncate_to_char_boundary(text: &str, max_bytes: usize) -> &str {
    if text.len() <= max_bytes {
        return text;
    }
    let cut = (0..=max_bytes)
        .rev()
        .find(|index| text.is_char_boundary(*index))
        .unwrap_or(0);
    &text[..cut]
}

fn truncate_chars(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let keep = max_chars.saturating_sub(3);
    let mut out = text.chars().take(keep).collect::<String>();
    out.push_str("...");
    out
}

#[cfg(test)]
pub(crate) fn write_spillover_for_test(id: &str, content: &str) -> AppResult<PathBuf> {
    let path = spillover_path_for_id(id)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&path, content)?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    struct ToolOutputRootGuard {
        prior: Option<String>,
        root: PathBuf,
    }

    impl Drop for ToolOutputRootGuard {
        fn drop(&mut self) {
            if let Some(prior) = self.prior.take() {
                std::env::set_var("DSCODE_TOOL_OUTPUT_ROOT", prior);
            } else {
                std::env::remove_var("DSCODE_TOOL_OUTPUT_ROOT");
            }
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    fn temp_root(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "deepseek-tool-output-{name}-{}-{nanos}",
            std::process::id()
        ))
    }

    fn with_tool_output_root(
        name: &str,
    ) -> (std::sync::MutexGuard<'static, ()>, ToolOutputRootGuard) {
        let lock = ENV_LOCK.lock().unwrap();
        let root = temp_root(name);
        fs::create_dir_all(&root).unwrap();
        let prior = std::env::var("DSCODE_TOOL_OUTPUT_ROOT").ok();
        std::env::set_var("DSCODE_TOOL_OUTPUT_ROOT", root.display().to_string());
        (lock, ToolOutputRootGuard { prior, root })
    }

    #[test]
    fn maybe_spill_successful_tool_output_writes_file_and_returns_hint() {
        let (_lock, guard) = with_tool_output_root("spill");
        let big = "a".repeat(SPILLOVER_THRESHOLD_BYTES + 16);
        let rendered = maybe_spill_successful_tool_output("run_shell", &big);
        assert!(rendered.contains("[large tool output spilled]"));
        assert!(rendered.contains("retrieve_tool_result ref=run_shell-"));
        let files = fs::read_dir(&guard.root).unwrap().collect::<Vec<_>>();
        assert_eq!(files.len(), 1);
    }

    #[test]
    fn retrieve_tool_result_summary_reads_by_id() {
        let (_lock, _guard) = with_tool_output_root("summary");
        write_spillover_for_test(
            "call-abc",
            "alpha\nwarning: careful\nbeta\nerror: broken\ntest result: FAILED\nomega",
        )
        .unwrap();
        let output = RetrieveToolResultTool
            .execute(ToolInput::new().with_arg("ref", "call-abc"))
            .unwrap();
        assert!(output.summary.contains("mode: summary"));
        assert!(output.summary.contains("signal_lines:"));
        assert!(output.summary.contains("warning: careful"));
        assert!(output.summary.contains("error: broken"));
    }

    #[test]
    fn retrieve_tool_result_query_returns_context() {
        let (_lock, _guard) = with_tool_output_root("query");
        write_spillover_for_test("call-query", "one\ntwo\nneedle here\nfour\nfive").unwrap();
        let output = RetrieveToolResultTool
            .execute(
                ToolInput::new()
                    .with_arg("ref", "tool_result:call-query")
                    .with_arg("mode", "query")
                    .with_arg("query", "needle")
                    .with_arg("context_lines", "1"),
            )
            .unwrap();
        assert!(output.summary.contains("matched_lines: 1"));
        assert!(output.summary.contains("2: two"));
        assert!(output.summary.contains("3: needle here"));
        assert!(output.summary.contains("4: four"));
    }

    #[test]
    fn retrieve_tool_result_lines_accepts_filename() {
        let (_lock, _guard) = with_tool_output_root("lines");
        write_spillover_for_test("call-lines", "a\nb\nc\nd").unwrap();
        let output = RetrieveToolResultTool
            .execute(
                ToolInput::new()
                    .with_arg("ref", "call-lines.txt")
                    .with_arg("mode", "lines")
                    .with_arg("lines", "2-3"),
            )
            .unwrap();
        assert!(output.summary.contains("2: b"));
        assert!(output.summary.contains("3: c"));
        assert!(!output.summary.contains("4: d"));
    }

    #[test]
    fn retrieve_tool_result_rejects_paths_outside_root() {
        let (_lock, _guard) = with_tool_output_root("outside");
        let outside = temp_root("outside-file");
        fs::write(&outside, "secret").unwrap();
        let err = RetrieveToolResultTool
            .execute(ToolInput::new().with_arg("ref", outside.display().to_string()))
            .unwrap_err();
        assert!(err.to_string().contains("does not point inside"));
        let _ = fs::remove_file(outside);
    }
}
