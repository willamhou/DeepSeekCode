use crate::config::types::DiagnosticsConfig;
use crate::error::app_error;
use crate::error::AppResult;
use crate::language::diagnostics::WarmDiagnosticSession;
use crate::tools::types::{Tool, ToolInput, ToolOutput};
use std::cell::RefCell;
use std::collections::BTreeSet;
use std::fs;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

pub struct ApplyPatchTool {
    pub diagnostics: DiagnosticsConfig,
    warm_diagnostics: RefCell<Option<WarmDiagnosticSession>>,
}

impl ApplyPatchTool {
    pub fn new(diagnostics: DiagnosticsConfig) -> Self {
        Self {
            diagnostics,
            warm_diagnostics: RefCell::new(None),
        }
    }
}

impl Default for ApplyPatchTool {
    fn default() -> Self {
        Self::new(DiagnosticsConfig::default())
    }
}

impl Tool for ApplyPatchTool {
    fn name(&self) -> &str {
        "apply_patch"
    }

    fn execute(&self, input: ToolInput) -> AppResult<ToolOutput> {
        if let Some(patch) = input.get("patch") {
            let cwd = input.get("cwd").unwrap_or(".");
            apply_unified_patch_with_session(
                cwd,
                patch,
                &self.diagnostics,
                Some(&self.warm_diagnostics),
            )
        } else {
            apply_text_replacement_with_session(
                &input,
                &self.diagnostics,
                Some(&self.warm_diagnostics),
            )
        }
    }
}

fn apply_text_replacement_with_session(
    input: &ToolInput,
    diagnostics: &DiagnosticsConfig,
    warm_session: Option<&RefCell<Option<WarmDiagnosticSession>>>,
) -> AppResult<ToolOutput> {
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

    let original = fs::read_to_string(path).map_err(|error| {
        if path.is_dir() {
            app_error(format!(
                "apply_patch path points to a directory: {}",
                path.display()
            ))
        } else {
            Box::new(error) as Box<dyn std::error::Error>
        }
    })?;
    let (updated, tolerant) = apply_replacement(&original, find, replace, replace_all)?;
    fs::write(path, updated)?;

    let mut summary = format!(
        "Updated {} using {} replacement mode{}.",
        path.display(),
        if replace_all { "global" } else { "single" },
        if tolerant {
            " (whitespace-tolerant match)"
        } else {
            ""
        }
    );
    summary.push_str(&format!(
        "\nmeta.replacement_fingerprint={}",
        replacement_fingerprint(find, replace)
    ));
    append_post_edit_diagnostics(
        &mut summary,
        Path::new("."),
        &[path.display().to_string()],
        diagnostics,
        warm_session,
    );

    Ok(ToolOutput { summary })
}

/// Returns the updated text and whether a whitespace-tolerant fallback was used.
fn apply_replacement(
    original: &str,
    find: &str,
    replace: &str,
    replace_all: bool,
) -> AppResult<(String, bool)> {
    if find.is_empty() {
        return Err(app_error("find string cannot be empty"));
    }

    if original.contains(find) {
        let updated = if replace_all {
            original.replace(find, replace)
        } else {
            original.replacen(find, replace, 1)
        };
        return Ok((updated, false));
    }

    // Exact match failed. Models frequently reproduce a find block that is
    // line-for-line correct after trimming but not byte-exact (indentation,
    // trailing spaces, or imperfectly stripped read_file line-number prefixes).
    // Most languages ignore that whitespace, so locating the right region is
    // what matters. Fall back to a trimmed line-block match, but only for a
    // single edit on LF text and only when exactly one location matches, so we
    // never guess at an ambiguous target.
    if !replace_all && !original.contains('\r') {
        if let Some(updated) = whitespace_tolerant_replacement(original, find, replace) {
            return Ok((updated, true));
        }
    }

    Err(app_error("find string not found in target file"))
}

/// Locate `find` in `original` comparing lines by trimmed content, and splice in
/// `replace`. Returns None unless exactly one line-block matches.
fn whitespace_tolerant_replacement(original: &str, find: &str, replace: &str) -> Option<String> {
    let find_lines: Vec<&str> = find.lines().collect();
    if find_lines.is_empty() {
        return None;
    }
    let original_lines: Vec<&str> = original.lines().collect();
    let window = find_lines.len();
    if window > original_lines.len() {
        return None;
    }

    let mut match_start: Option<usize> = None;
    for start in 0..=(original_lines.len() - window) {
        let block_matches = (0..window)
            .all(|offset| original_lines[start + offset].trim() == find_lines[offset].trim());
        if block_matches {
            if match_start.is_some() {
                // Ambiguous: more than one trimmed match. Refuse to guess.
                return None;
            }
            match_start = Some(start);
        }
    }

    let start = match_start?;
    let mut result: Vec<&str> = Vec::with_capacity(original_lines.len());
    result.extend_from_slice(&original_lines[..start]);
    result.extend(replace.lines());
    result.extend_from_slice(&original_lines[start + window..]);
    let mut updated = result.join("\n");
    if original.ends_with('\n') {
        updated.push('\n');
    }
    Some(updated)
}

fn replacement_fingerprint(find: &str, replace: &str) -> String {
    let mut canonical = String::new();
    push_fingerprint_part(&mut canonical, "find", find);
    push_fingerprint_part(&mut canonical, "replace", replace);
    format!("{:016x}", fnv1a64(canonical.as_bytes()))
}

fn push_fingerprint_part(out: &mut String, key: &str, value: &str) {
    out.push_str(key);
    out.push('=');
    out.push_str(&value.len().to_string());
    out.push(':');
    out.push_str(value);
    out.push('\n');
}

fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

#[cfg(test)]
fn apply_unified_patch(
    cwd: &str,
    patch: &str,
    diagnostics: &DiagnosticsConfig,
) -> AppResult<ToolOutput> {
    apply_unified_patch_with_session(cwd, patch, diagnostics, None)
}

fn apply_unified_patch_with_session(
    cwd: &str,
    patch: &str,
    diagnostics: &DiagnosticsConfig,
    warm_session: Option<&RefCell<Option<WarmDiagnosticSession>>>,
) -> AppResult<ToolOutput> {
    if patch.trim().is_empty() {
        return Err(app_error("patch content cannot be empty"));
    }

    let canonical_cwd = fs::canonicalize(cwd).ok();
    let patch_body = normalize_patch_paths(canonical_cwd.as_deref(), patch);
    let summary = summarize_patch(&patch_body)?;

    if summary.is_empty() {
        return Err(app_error(
            "patch did not declare any file headers (`--- ` / `+++ `)",
        ));
    }

    let canonical_cwd = canonical_cwd.unwrap_or_else(|| PathBuf::from(cwd));
    validate_patch_scope(&canonical_cwd, &summary)?;

    let temp_path = unique_patch_path();
    let patch_body = ensure_trailing_newline(&patch_body);
    fs::write(&temp_path, &patch_body)?;

    let dry_run = run_patch_cli(cwd, &temp_path, true)?;
    if !dry_run.status.success() {
        let _ = fs::remove_file(&temp_path);
        return Err(app_error(format_patch_failure(
            "patch dry-run failed",
            &dry_run.stdout,
            &dry_run.stderr,
        )));
    }

    let apply = run_patch_cli(cwd, &temp_path, false)?;
    let _ = fs::remove_file(&temp_path);

    if !apply.status.success() {
        return Err(app_error(format_patch_failure(
            "patch apply failed",
            &apply.stdout,
            &apply.stderr,
        )));
    }

    let affected_paths = summary.affected_paths_owned();
    let mut output_summary = format_success_summary(cwd, &summary, &apply.stdout, &apply.stderr);
    append_post_edit_diagnostics(
        &mut output_summary,
        Path::new(cwd),
        &affected_paths,
        diagnostics,
        warm_session,
    );

    Ok(ToolOutput {
        summary: output_summary,
    })
}

fn append_post_edit_diagnostics(
    summary: &mut String,
    cwd: &Path,
    files: &[String],
    diagnostics: &DiagnosticsConfig,
    warm_session: Option<&RefCell<Option<WarmDiagnosticSession>>>,
) {
    if !diagnostics.post_edit {
        return;
    }
    let report = match warm_session {
        Some(warm_session) => run_warmed_post_edit_diagnostics(cwd, files, warm_session),
        None => crate::language::diagnostics::run_diagnostics(cwd, files),
    };
    summary.push_str("\n\npost-edit diagnostics:\n");
    summary.push_str(&report.render_text());
}

fn run_warmed_post_edit_diagnostics(
    cwd: &Path,
    files: &[String],
    warm_session: &RefCell<Option<WarmDiagnosticSession>>,
) -> crate::language::diagnostics::DiagnosticReport {
    let cwd = fs::canonicalize(cwd).unwrap_or_else(|_| cwd.to_path_buf());
    let mut slot = warm_session.borrow_mut();
    let reset = slot
        .as_ref()
        .map(|session| session.cwd() != cwd.as_path())
        .unwrap_or(true);
    if reset {
        *slot = Some(WarmDiagnosticSession::new(cwd.clone(), files));
    }
    slot.as_mut()
        .expect("warm diagnostic session initialized")
        .run(files)
}

fn run_patch_cli(cwd: &str, patch_path: &Path, dry_run: bool) -> std::io::Result<PatchOutput> {
    let mut command = Command::new("patch");
    command.args(["--batch", "--forward", "--binary", "-p0"]);
    if dry_run {
        command.arg("--dry-run");
    }
    let output = command
        .arg("-i")
        .arg(patch_path)
        .current_dir(cwd)
        .output()?;

    Ok(PatchOutput {
        status: output.status,
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    })
}

struct PatchOutput {
    status: std::process::ExitStatus,
    stdout: String,
    stderr: String,
}

fn ensure_trailing_newline(value: &str) -> String {
    if value.ends_with('\n') {
        value.to_string()
    } else {
        format!("{value}\n")
    }
}

fn normalize_patch_paths(canonical_cwd: Option<&Path>, patch: &str) -> String {
    let mut output = String::with_capacity(patch.len() + 8);
    for (index, line) in patch.split('\n').enumerate() {
        if index > 0 {
            output.push('\n');
        }
        normalize_patch_header_into(&mut output, canonical_cwd, line);
    }
    output
}

fn normalize_patch_header_into(output: &mut String, cwd: Option<&Path>, line: &str) {
    let Some((prefix, raw_path)) = line
        .strip_prefix("--- ")
        .map(|path| ("--- ", path))
        .or_else(|| line.strip_prefix("+++ ").map(|path| ("+++ ", path)))
    else {
        output.push_str(line);
        return;
    };

    let path_token = raw_path.split_whitespace().next().unwrap_or(raw_path);
    if path_token == "/dev/null" {
        output.push_str(prefix);
        output.push_str(path_token);
        return;
    }

    let stripped = strip_git_prefix(path_token);
    let path = Path::new(stripped);
    if path.is_absolute() {
        if let Some(cwd) = cwd {
            if let Ok(relative) = path.strip_prefix(cwd) {
                output.push_str(prefix);
                output.push_str(&relative.display().to_string());
                return;
            }
        }
        output.push_str(prefix);
        output.push_str(&path.display().to_string());
        return;
    }

    output.push_str(prefix);
    output.push_str(stripped);
}

fn strip_git_prefix(path: &str) -> &str {
    path.strip_prefix("a/")
        .or_else(|| path.strip_prefix("b/"))
        .unwrap_or(path)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PatchPath {
    DevNull,
    Relative(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatchTarget {
    pub old: PatchPath,
    pub new: PatchPath,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PatchSummary {
    pub targets: Vec<PatchTarget>,
}

impl PatchSummary {
    pub fn is_empty(&self) -> bool {
        self.targets.is_empty()
    }

    pub fn creates(&self) -> Vec<&str> {
        self.targets
            .iter()
            .filter_map(|target| match (&target.old, &target.new) {
                (PatchPath::DevNull, PatchPath::Relative(path)) => Some(path.as_str()),
                _ => None,
            })
            .collect()
    }

    pub fn deletes(&self) -> Vec<&str> {
        self.targets
            .iter()
            .filter_map(|target| match (&target.old, &target.new) {
                (PatchPath::Relative(path), PatchPath::DevNull) => Some(path.as_str()),
                _ => None,
            })
            .collect()
    }

    pub fn modifies(&self) -> Vec<&str> {
        self.targets
            .iter()
            .filter_map(|target| match (&target.old, &target.new) {
                (PatchPath::Relative(old), PatchPath::Relative(new)) if old == new => {
                    Some(new.as_str())
                }
                _ => None,
            })
            .collect()
    }

    pub fn renames(&self) -> Vec<(&str, &str)> {
        self.targets
            .iter()
            .filter_map(|target| match (&target.old, &target.new) {
                (PatchPath::Relative(old), PatchPath::Relative(new)) if old != new => {
                    Some((old.as_str(), new.as_str()))
                }
                _ => None,
            })
            .collect()
    }

    fn affected_paths(&self) -> BTreeSet<&str> {
        let mut set = BTreeSet::new();
        for target in &self.targets {
            if let PatchPath::Relative(path) = &target.old {
                set.insert(path.as_str());
            }
            if let PatchPath::Relative(path) = &target.new {
                set.insert(path.as_str());
            }
        }
        set
    }

    fn affected_paths_owned(&self) -> Vec<String> {
        self.affected_paths()
            .into_iter()
            .map(str::to_string)
            .collect()
    }
}

fn summarize_patch(patch: &str) -> AppResult<PatchSummary> {
    let mut targets = Vec::new();
    let mut pending_old: Option<PatchPath> = None;

    for line in patch.lines() {
        if let Some(rest) = line.strip_prefix("--- ") {
            pending_old = Some(parse_header_path(rest)?);
            continue;
        }

        if let Some(rest) = line.strip_prefix("+++ ") {
            let new_path = parse_header_path(rest)?;
            let old_path = pending_old.take().ok_or_else(|| {
                app_error("patch contains `+++ ` header without preceding `--- `")
            })?;
            targets.push(PatchTarget {
                old: old_path,
                new: new_path,
            });
        }
    }

    Ok(PatchSummary { targets })
}

fn parse_header_path(rest: &str) -> AppResult<PatchPath> {
    let token = rest.split_whitespace().next().unwrap_or("");
    if token.is_empty() {
        return Err(app_error("patch header missing path"));
    }
    if token == "/dev/null" {
        return Ok(PatchPath::DevNull);
    }
    let stripped = strip_git_prefix(token);
    Ok(PatchPath::Relative(stripped.to_string()))
}

fn validate_patch_scope(cwd: &Path, summary: &PatchSummary) -> AppResult<()> {
    for path in summary.affected_paths() {
        ensure_within_cwd(cwd, path)?;
    }
    Ok(())
}

fn ensure_within_cwd(cwd: &Path, raw: &str) -> AppResult<()> {
    let candidate = Path::new(raw);
    if candidate.is_absolute() {
        return Err(app_error(format!(
            "patch target `{raw}` must be relative to cwd"
        )));
    }

    if raw.split(['/', '\\']).any(|segment| segment == "..") {
        return Err(app_error(format!(
            "patch target `{raw}` escapes cwd via parent reference"
        )));
    }

    let joined = cwd.join(candidate);
    let canonical_target = fs::canonicalize(&joined).ok();
    if let Some(target) = canonical_target {
        if !target.starts_with(cwd) {
            return Err(app_error(format!(
                "patch target `{raw}` resolves outside cwd"
            )));
        }
    }

    Ok(())
}

fn format_success_summary(cwd: &str, summary: &PatchSummary, stdout: &str, stderr: &str) -> String {
    let mut output = String::new();
    output.push_str(&format!(
        "Applied unified patch in {cwd} (touched {} file{}).",
        summary.targets.len(),
        if summary.targets.len() == 1 { "" } else { "s" }
    ));

    let creates = summary.creates();
    let deletes = summary.deletes();
    let modifies = summary.modifies();
    let renames = summary.renames();

    if !modifies.is_empty() {
        output.push_str("\nmodified:");
        for path in modifies {
            output.push_str(&format!("\n  - {path}"));
        }
    }
    if !creates.is_empty() {
        output.push_str("\ncreated:");
        for path in creates {
            output.push_str(&format!("\n  + {path}"));
        }
    }
    if !deletes.is_empty() {
        output.push_str("\ndeleted:");
        for path in deletes {
            output.push_str(&format!("\n  - {path}"));
        }
    }
    if !renames.is_empty() {
        output.push_str("\nrenamed:");
        for (old, new) in renames {
            output.push_str(&format!("\n  {old} -> {new}"));
        }
    }

    let stdout_trimmed = stdout.trim();
    if !stdout_trimmed.is_empty() {
        output.push_str("\nstdout:\n");
        output.push_str(stdout_trimmed);
    }
    let stderr_trimmed = stderr.trim();
    if !stderr_trimmed.is_empty() {
        output.push_str("\nstderr:\n");
        output.push_str(stderr_trimmed);
    }

    output
}

fn format_patch_failure(prefix: &str, stdout: &str, stderr: &str) -> String {
    let detail = if !stderr.trim().is_empty() {
        stderr.trim()
    } else {
        stdout.trim()
    };

    let category = classify_patch_failure(stdout, stderr);
    match category {
        Some(reason) => format!("{prefix}: {reason}\nraw: {detail}"),
        None => format!("{prefix}: {detail}"),
    }
}

fn classify_patch_failure(stdout: &str, stderr: &str) -> Option<String> {
    let combined = format!("{stdout}\n{stderr}").to_lowercase();

    if combined.contains("can't find file to patch")
        || combined.contains("no file to patch")
        || combined.contains("no such file or directory")
    {
        return Some(
            "target file does not exist (patch references a path that is not in cwd)".to_string(),
        );
    }
    if combined.contains("reversed (or previously applied) patch") {
        return Some(
            "patch appears to be already applied or reversed; pass it through `git apply -R` if you intend to revert".to_string(),
        );
    }
    if let Some(hunk_index) = find_failed_hunk_index(&combined) {
        return Some(format!(
            "hunk #{hunk_index} did not match the target file (the surrounding context drifted; re-read the file before patching)"
        ));
    }
    if combined.contains("malformed patch") {
        return Some(
            "patch is malformed (check `--- ` / `+++ ` / `@@` headers and indentation)".to_string(),
        );
    }
    if combined.contains("only garbage was found") {
        return Some(
            "no patch headers were recognized (ensure the body uses unified diff format)"
                .to_string(),
        );
    }
    None
}

fn find_failed_hunk_index(combined_lower: &str) -> Option<u32> {
    let marker = "hunk #";
    let mut cursor = 0;
    while let Some(local) = combined_lower[cursor..].find(marker) {
        let absolute = cursor + local + marker.len();
        let rest = &combined_lower[absolute..];
        let digits: String = rest.chars().take_while(|ch| ch.is_ascii_digit()).collect();
        if !digits.is_empty() {
            let after_digits = &rest[digits.len()..];
            if after_digits.contains("failed") {
                if let Ok(value) = digits.parse() {
                    return Some(value);
                }
            }
        }
        cursor = absolute + digits.len().max(1);
    }
    None
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnifiedDiffPlan {
    pub cwd: String,
    pub patch: String,
}

pub fn build_single_line_diff(path: &str, find: &str, replace: &str) -> Option<UnifiedDiffPlan> {
    if find.is_empty() || find.contains('\n') || replace.contains('\n') {
        return None;
    }

    let file = File::open(path).ok()?;
    let mut reader = BufReader::new(file);
    let mut buffer = String::new();
    let mut first_match: Option<(usize, String)> = None;
    let mut index = 0usize;

    loop {
        buffer.clear();
        let bytes = reader.read_line(&mut buffer).ok()?;
        if bytes == 0 {
            break;
        }
        let line = buffer.strip_suffix('\n').unwrap_or(buffer.as_str());
        if line.contains(find) {
            if first_match.is_some() {
                return None;
            }
            if line.matches(find).count() != 1 {
                return None;
            }
            first_match = Some((index, line.to_string()));
        }
        index += 1;
    }

    let (first_index, first_line) = first_match?;
    let line_number = first_index + 1;
    let new_line = first_line.replacen(find, replace, 1);

    let (cwd, header_path) = split_path_for_patch(path);

    let patch = format!(
        "--- {header_path}\n+++ {header_path}\n@@ -{line_number},1 +{line_number},1 @@\n-{first_line}\n+{new_line}\n",
    );

    Some(UnifiedDiffPlan { cwd, patch })
}

fn split_path_for_patch(path: &str) -> (String, String) {
    let path_obj = Path::new(path);
    let parent = path_obj
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .map(|parent| parent.to_string_lossy().into_owned())
        .unwrap_or_else(|| ".".to_string());
    let file_name = path_obj
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string());
    (parent, file_name)
}

fn unique_patch_path() -> std::path::PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    std::env::temp_dir().join(format!("dscode_patch_{nanos}.diff"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn replaces_first_occurrence_only() {
        let (updated, tolerant) = apply_replacement("a b a", "a", "x", false).unwrap();
        assert_eq!(updated, "x b a");
        assert!(!tolerant, "exact match must not report tolerant fallback");
    }

    #[test]
    fn replaces_all_occurrences() {
        let (updated, tolerant) = apply_replacement("a b a", "a", "x", true).unwrap();
        assert_eq!(updated, "x b x");
        assert!(!tolerant);
    }

    #[test]
    fn errors_when_find_is_missing() {
        let error = apply_replacement("hello", "missing", "x", false).unwrap_err();
        assert!(error.to_string().contains("not found"));
    }

    #[test]
    fn tolerant_match_applies_across_indentation_difference() {
        // find reproduces the lines but with the wrong (zero) indentation, as a
        // model often does after stripping read_file's line-number prefix.
        let original = "fn f() {\n    let x = 1;\n    x\n}\n";
        let find = "let x = 1;\nx";
        let replace = "let x = 2;\n    x";
        let (updated, tolerant) = apply_replacement(original, find, replace, false).unwrap();
        assert!(tolerant, "indentation-only mismatch should use tolerant path");
        assert!(updated.contains("let x = 2;"));
        assert!(!updated.contains("let x = 1;"));
        assert!(updated.ends_with('\n'), "trailing newline must be preserved");
    }

    #[test]
    fn tolerant_match_refuses_ambiguous_target() {
        // Two trimmed-equal blocks: refuse to guess which one to edit.
        let original = "if x:\n    pass\nif x:\n    pass\n";
        let find = "if x:\npass";
        let error = apply_replacement(original, find, "CHANGED", false).unwrap_err();
        assert!(error.to_string().contains("not found"));
    }

    #[test]
    fn tolerant_match_skipped_for_crlf_files() {
        // CRLF files are left to exact matching so the fallback never rewrites
        // line endings across the whole file.
        let original = "  alpha\r\n  beta\r\n";
        let find = "alpha\nbeta";
        let error = apply_replacement(original, find, "X", false).unwrap_err();
        assert!(error.to_string().contains("not found"));
    }

    #[test]
    fn tolerant_match_not_used_for_replace_all() {
        // Global replace stays strict; a non-exact find errors instead of
        // tolerant-matching at one site.
        let original = "  a = 1\n  a = 1\n";
        let find = "a = 1\na = 1";
        let error = apply_replacement(original, find, "X", true).unwrap_err();
        assert!(error.to_string().contains("not found"));
    }

    #[test]
    fn text_replacement_summary_includes_replacement_fingerprint() {
        let dir = unique_test_dir();
        fs::create_dir_all(&dir).unwrap();
        let file = dir.join("demo.txt");
        fs::write(&file, "alpha\nbeta\n").unwrap();

        let output = apply_text_replacement_with_session(
            &ToolInput::new()
                .with_arg("path", file.to_string_lossy().into_owned())
                .with_arg("find", "alpha")
                .with_arg("replace", "omega"),
            &DiagnosticsConfig::default(),
            None,
        )
        .unwrap();

        assert!(output.summary.contains("Updated "));
        assert!(output.summary.contains("meta.replacement_fingerprint="));
        assert_eq!(fs::read_to_string(&file).unwrap(), "omega\nbeta\n");

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn applies_unified_diff_patch() {
        let dir = unique_test_dir();
        fs::create_dir_all(&dir).unwrap();
        let file = dir.join("demo.txt");
        fs::write(&file, "alpha\nbeta\n").unwrap();

        let patch = format!(
            "--- {0}\n+++ {0}\n@@ -1,2 +1,2 @@\n-alpha\n+omega\n beta\n",
            file.display()
        );

        let summary =
            apply_unified_patch(dir.to_str().unwrap(), &patch, &DiagnosticsConfig::default())
                .unwrap();
        let content = fs::read_to_string(&file).unwrap();

        assert!(summary.summary.contains("Applied unified patch"));
        assert!(summary.summary.contains("touched 1 file"));
        assert!(summary.summary.contains("modified:"));
        assert_eq!(content, "omega\nbeta\n");

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn applies_multi_file_patch_and_lists_each() {
        let dir = unique_test_dir();
        fs::create_dir_all(&dir).unwrap();
        let one = dir.join("one.txt");
        let two = dir.join("two.txt");
        fs::write(&one, "one\n").unwrap();
        fs::write(&two, "two\n").unwrap();

        let patch = format!(
            "--- {0}\n+++ {0}\n@@ -1 +1 @@\n-one\n+ONE\n--- {1}\n+++ {1}\n@@ -1 +1 @@\n-two\n+TWO\n",
            one.display(),
            two.display(),
        );

        let result =
            apply_unified_patch(dir.to_str().unwrap(), &patch, &DiagnosticsConfig::default())
                .unwrap();
        assert!(result.summary.contains("touched 2 files"));
        assert!(result.summary.contains("one.txt"));
        assert!(result.summary.contains("two.txt"));

        assert_eq!(fs::read_to_string(&one).unwrap(), "ONE\n");
        assert_eq!(fs::read_to_string(&two).unwrap(), "TWO\n");

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn post_edit_diagnostics_can_be_enabled_for_unified_patch() {
        let dir = unique_test_dir();
        fs::create_dir_all(&dir).unwrap();
        let file = dir.join("demo.txt");
        fs::write(&file, "alpha\n").unwrap();
        let patch = "--- demo.txt\n+++ demo.txt\n@@ -1 +1 @@\n-alpha\n+beta\n";
        let diagnostics = DiagnosticsConfig { post_edit: true };

        let result = apply_unified_patch(dir.to_str().unwrap(), patch, &diagnostics).unwrap();

        assert!(result.summary.contains("post-edit diagnostics:"));
        assert!(result.summary.contains("diagnostics: unavailable"));
        assert_eq!(fs::read_to_string(&file).unwrap(), "beta\n");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn apply_patch_tool_uses_warmed_post_edit_diagnostics_path() {
        let dir = unique_test_dir();
        fs::create_dir_all(&dir).unwrap();
        let file = dir.join("demo.txt");
        fs::write(&file, "alpha\n").unwrap();
        let patch = "--- demo.txt\n+++ demo.txt\n@@ -1 +1 @@\n-alpha\n+beta\n";
        let tool = ApplyPatchTool::new(DiagnosticsConfig { post_edit: true });

        let result = tool
            .execute(
                ToolInput::new()
                    .with_arg("cwd", dir.display().to_string())
                    .with_arg("patch", patch),
            )
            .unwrap();

        assert!(result.summary.contains("post-edit diagnostics:"));
        assert!(result.summary.contains("diagnostics: unavailable"));
        assert_eq!(fs::read_to_string(&file).unwrap(), "beta\n");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn rejects_patch_with_parent_escape() {
        let dir = unique_test_dir();
        fs::create_dir_all(&dir).unwrap();

        let patch = "--- a/../escape.txt\n+++ b/../escape.txt\n@@ -1 +1 @@\n-x\n+y\n";
        let error =
            apply_unified_patch(dir.to_str().unwrap(), patch, &DiagnosticsConfig::default())
                .unwrap_err();
        assert!(error.to_string().contains("escapes cwd"));

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn rejects_patch_with_no_headers() {
        let dir = unique_test_dir();
        fs::create_dir_all(&dir).unwrap();

        let patch = "garbage without headers\n";
        let error =
            apply_unified_patch(dir.to_str().unwrap(), patch, &DiagnosticsConfig::default())
                .unwrap_err();
        assert!(error
            .to_string()
            .contains("did not declare any file headers"));

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn diagnoses_missing_file() {
        let dir = unique_test_dir();
        fs::create_dir_all(&dir).unwrap();

        let patch = "--- ghost.txt\n+++ ghost.txt\n@@ -1 +1 @@\n-ghost\n+ghost!\n";
        let error =
            apply_unified_patch(dir.to_str().unwrap(), patch, &DiagnosticsConfig::default())
                .unwrap_err();
        let message = error.to_string();
        assert!(message.contains("does not exist"), "got: {message}");

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn parse_header_path_strips_git_prefix() {
        assert_eq!(
            parse_header_path("a/src/main.rs").unwrap(),
            PatchPath::Relative("src/main.rs".to_string())
        );
        assert_eq!(
            parse_header_path("b/src/main.rs").unwrap(),
            PatchPath::Relative("src/main.rs".to_string())
        );
        assert_eq!(parse_header_path("/dev/null").unwrap(), PatchPath::DevNull);
    }

    #[test]
    fn summarize_patch_classifies_create_delete_modify() {
        let patch = concat!(
            "--- /dev/null\n",
            "+++ b/new.txt\n",
            "@@ -0,0 +1 @@\n",
            "+hi\n",
            "--- a/old.txt\n",
            "+++ /dev/null\n",
            "@@ -1 +0,0 @@\n",
            "-bye\n",
            "--- a/keep.txt\n",
            "+++ b/keep.txt\n",
            "@@ -1 +1 @@\n",
            "-x\n",
            "+y\n",
        );
        let summary = summarize_patch(patch).unwrap();
        assert_eq!(summary.creates(), vec!["new.txt"]);
        assert_eq!(summary.deletes(), vec!["old.txt"]);
        assert_eq!(summary.modifies(), vec!["keep.txt"]);
    }

    #[test]
    fn classify_failure_recognizes_missing_file() {
        let category = classify_patch_failure("", "patch: **** can't find file to patch");
        assert!(category.unwrap().contains("does not exist"));
    }

    #[test]
    fn classify_failure_recognizes_macos_missing_file() {
        let category = classify_patch_failure(
            "No file to patch.  Skipping...\n1 out of 1 hunks ignored while patching ghost.txt",
            "",
        );
        assert!(category.unwrap().contains("does not exist"));
    }

    #[test]
    fn classify_failure_recognizes_failed_hunk() {
        let category = classify_patch_failure("Hunk #2 FAILED at 17.\n", "");
        assert!(category.unwrap().contains("hunk #2"));
    }

    #[test]
    fn classify_failure_recognizes_already_applied() {
        let category =
            classify_patch_failure("Reversed (or previously applied) patch detected!", "");
        assert!(category.unwrap().contains("already applied"));
    }

    #[test]
    fn ensure_within_cwd_rejects_absolute_paths() {
        let cwd = std::env::temp_dir();
        let error = ensure_within_cwd(&cwd, "/etc/passwd").unwrap_err();
        assert!(error.to_string().contains("must be relative"));
    }

    #[test]
    fn ensure_within_cwd_rejects_parent_segments() {
        let cwd = std::env::temp_dir();
        let error = ensure_within_cwd(&cwd, "../escape").unwrap_err();
        assert!(error.to_string().contains("escapes cwd"));
    }

    #[test]
    fn build_single_line_diff_emits_valid_hunk_for_substring() {
        let dir = unique_test_dir();
        fs::create_dir_all(&dir).unwrap();
        let file = dir.join("note.txt");
        fs::write(&file, "alpha\nbeta gamma\ndelta\n").unwrap();

        let plan = build_single_line_diff(file.to_str().unwrap(), "gamma", "GAMMA").unwrap();

        assert_eq!(plan.cwd, dir.to_string_lossy());
        assert!(plan.patch.contains("--- note.txt"));
        assert!(plan.patch.contains("+++ note.txt"));
        assert!(plan.patch.contains("@@ -2,1 +2,1 @@"));
        assert!(plan.patch.contains("-beta gamma"));
        assert!(plan.patch.contains("+beta GAMMA"));

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn build_single_line_diff_returns_none_when_find_missing() {
        let dir = unique_test_dir();
        fs::create_dir_all(&dir).unwrap();
        let file = dir.join("note.txt");
        fs::write(&file, "alpha\nbeta\n").unwrap();

        let patch = build_single_line_diff(file.to_str().unwrap(), "ghost", "x");
        assert!(patch.is_none());

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn build_single_line_diff_returns_none_for_multiline_find() {
        let dir = unique_test_dir();
        fs::create_dir_all(&dir).unwrap();
        let file = dir.join("note.txt");
        fs::write(&file, "alpha\nbeta\n").unwrap();

        let patch = build_single_line_diff(file.to_str().unwrap(), "alpha\nbeta", "x");
        assert!(patch.is_none());

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn build_single_line_diff_returns_none_when_ambiguous() {
        let dir = unique_test_dir();
        fs::create_dir_all(&dir).unwrap();
        let file = dir.join("note.txt");
        fs::write(&file, "alpha\nalpha\n").unwrap();

        let patch = build_single_line_diff(file.to_str().unwrap(), "alpha", "x");
        assert!(patch.is_none());

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn build_single_line_diff_round_trips_for_crlf_files() {
        let dir = unique_test_dir();
        fs::create_dir_all(&dir).unwrap();
        let file = dir.join("note.txt");
        fs::write(&file, "alpha\r\nbeta gamma\r\ndelta\r\n").unwrap();

        let plan = build_single_line_diff(file.to_str().unwrap(), "gamma", "GAMMA").unwrap();
        apply_unified_patch(&plan.cwd, &plan.patch, &DiagnosticsConfig::default()).unwrap();
        assert_eq!(
            fs::read_to_string(&file).unwrap(),
            "alpha\r\nbeta GAMMA\r\ndelta\r\n"
        );

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn build_single_line_diff_round_trips_through_apply_unified_patch() {
        let dir = unique_test_dir();
        fs::create_dir_all(&dir).unwrap();
        let file = dir.join("note.txt");
        fs::write(&file, "alpha\nbeta gamma\ndelta\n").unwrap();

        let plan = build_single_line_diff(file.to_str().unwrap(), "gamma", "GAMMA").unwrap();

        apply_unified_patch(&plan.cwd, &plan.patch, &DiagnosticsConfig::default()).unwrap();
        assert_eq!(
            fs::read_to_string(&file).unwrap(),
            "alpha\nbeta GAMMA\ndelta\n"
        );

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn split_path_for_patch_handles_absolute_and_bare_names() {
        let (cwd, name) = split_path_for_patch("/tmp/foo/note.txt");
        assert_eq!(cwd, "/tmp/foo");
        assert_eq!(name, "note.txt");

        let (cwd, name) = split_path_for_patch("note.txt");
        assert_eq!(cwd, ".");
        assert_eq!(name, "note.txt");

        let (cwd, name) = split_path_for_patch("dir/note.txt");
        assert_eq!(cwd, "dir");
        assert_eq!(name, "note.txt");
    }

    fn unique_test_dir() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let base = std::env::temp_dir()
            .canonicalize()
            .unwrap_or_else(|_| std::env::temp_dir());
        base.join(format!("dscode_patch_test_{nanos}"))
    }
}
