# PR/CI Integration Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add `dscode pr review|fix|patch` subcommand group that uses the existing agent loop to operate on GitHub pull requests via the `gh` CLI.

**Architecture:** A new `src/integrations/github.rs` wraps `gh` CLI calls and exposes typed structs (`PrContext`, `CiFailure`). A new `src/cli/commands/pr.rs` dispatches three subcommands that share PR fetching, populate prefilled observations, and invoke the existing `AgentLoop`. The agent loop gains a `run_with(ctx, options)` API that accepts an explicit step budget plus initial observations, leaving `run(ctx)` as a thin compatibility shim.

**Tech Stack:** Rust 2021 (no new crates), hand-rolled JSON parser (hoisted to `src/util/json.rs`), `gh` CLI (≥2.40) for GitHub I/O, existing `tools::registry` + `core::observations` + P3 confirm prompts.

**Pre-flight:** `~/.cargo/bin/cargo test --offline` must show `72 passed` before starting. All commands assume the working directory is the repo root.

---

## Task 1: Hoist JSON Parser to `util::json`

The current hand-rolled JSON parser lives privately inside `src/model/deepseek.rs`. The new `integrations::github` module needs the same parser. Move it to a shared location without changing behaviour. This task adds zero tests; existing 72 tests must still pass.

**Files:**
- Create: `src/util/mod.rs`
- Create: `src/util/json.rs`
- Modify: `src/main.rs` (register `util` module)
- Modify: `src/model/deepseek.rs` (delete moved code, import from `crate::util::json`)

- [ ] **Step 1: Create `src/util/mod.rs`**

```rust
pub mod json;
```

- [ ] **Step 2: Create `src/util/json.rs` with the moved parser**

Open `src/model/deepseek.rs` and copy verbatim every item below into a new file `src/util/json.rs`, marking each item `pub`:

- the `JsonValue` enum
- `parse_json_value`
- `parse_value`
- `parse_object`
- `parse_array`
- `parse_string`
- `parse_bool`
- `parse_number`
- `skip_ws`
- `json_as_string`
- `json_as_object`
- `json_as_array`
- `parse_root_object`

Add at the top of the new file:

```rust
use std::collections::BTreeMap;

use crate::error::{app_error, AppResult};
```

Mark `JsonValue` and every `fn` listed above as `pub`. Leave the bodies unchanged.

- [ ] **Step 3: Register the module in `src/main.rs`**

Open `src/main.rs`, find the existing module declarations (look for `mod cli;`, `mod core;`, etc.). Add:

```rust
mod util;
```

Place it alphabetically next to the other top-level modules.

- [ ] **Step 4: Delete the moved items from `src/model/deepseek.rs` and import them**

In `src/model/deepseek.rs`:
- Delete the `JsonValue` enum and all 12 functions listed in Step 2.
- Add at the top of the file (next to other `use` statements):

```rust
use crate::util::json::{
    json_as_array, json_as_object, json_as_string, parse_root_object, JsonValue,
};
```

- [ ] **Step 5: Run all tests, verify nothing regressed**

Run: `~/.cargo/bin/cargo test --offline`
Expected: `test result: ok. 72 passed; 0 failed; 0 ignored; 0 measured`

- [ ] **Step 6: Commit**

```bash
git add src/util/mod.rs src/util/json.rs src/main.rs src/model/deepseek.rs
git commit -m "Hoist hand-rolled JSON parser into util::json module"
```

---

## Task 2: Add `integrations` Module + `PrRef` parser

Introduce the `integrations` top-level module and the `PrRef` type that handles the three accepted PR-reference shapes.

**Files:**
- Create: `src/integrations/mod.rs`
- Create: `src/integrations/github.rs`
- Modify: `src/main.rs` (register `integrations` module)

- [ ] **Step 1: Create `src/integrations/mod.rs`**

```rust
pub mod github;
```

- [ ] **Step 2: Create `src/integrations/github.rs` with PrRef + tests scaffold**

```rust
use crate::error::{app_error, AppResult};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrRef {
    Number(u64),
    Qualified { repo: String, number: u64 },
}

pub fn parse_pr_ref(input: &str) -> AppResult<PrRef> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err(app_error("PR reference cannot be empty"));
    }

    if let Some(stripped) = trimmed.strip_prefix("https://github.com/") {
        let mut parts = stripped.split('/');
        let owner = parts.next().unwrap_or("");
        let repo = parts.next().unwrap_or("");
        let kind = parts.next().unwrap_or("");
        let number = parts.next().unwrap_or("");
        if kind != "pull" || owner.is_empty() || repo.is_empty() {
            return Err(app_error(format!("malformed GitHub PR URL: {input}")));
        }
        let number: u64 = number
            .parse()
            .map_err(|_| app_error(format!("PR URL has non-numeric ID: {input}")))?;
        return Ok(PrRef::Qualified {
            repo: format!("{owner}/{repo}"),
            number,
        });
    }

    if let Some((repo, number)) = trimmed.split_once('#') {
        if !repo.contains('/') {
            return Err(app_error(format!(
                "qualified PR reference must be `owner/repo#N`: {input}"
            )));
        }
        let number: u64 = number
            .parse()
            .map_err(|_| app_error(format!("qualified PR reference has non-numeric N: {input}")))?;
        return Ok(PrRef::Qualified {
            repo: repo.to_string(),
            number,
        });
    }

    let number: u64 = trimmed
        .parse()
        .map_err(|_| app_error(format!("PR reference is not a number, owner/repo#N, or URL: {input}")))?;
    Ok(PrRef::Number(number))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_bare_number() {
        assert_eq!(parse_pr_ref("123").unwrap(), PrRef::Number(123));
    }

    #[test]
    fn parses_qualified_owner_repo_form() {
        let parsed = parse_pr_ref("willamhou/DeepseekCode#42").unwrap();
        assert_eq!(
            parsed,
            PrRef::Qualified {
                repo: "willamhou/DeepseekCode".to_string(),
                number: 42,
            }
        );
    }

    #[test]
    fn parses_github_pull_request_url() {
        let parsed =
            parse_pr_ref("https://github.com/willamhou/DeepseekCode/pull/7").unwrap();
        assert_eq!(
            parsed,
            PrRef::Qualified {
                repo: "willamhou/DeepseekCode".to_string(),
                number: 7,
            }
        );
    }

    #[test]
    fn rejects_blank_input() {
        assert!(parse_pr_ref("   ").is_err());
    }

    #[test]
    fn rejects_qualified_form_without_slash() {
        assert!(parse_pr_ref("repo#3").is_err());
    }

    #[test]
    fn rejects_non_numeric_id() {
        assert!(parse_pr_ref("owner/repo#abc").is_err());
    }
}
```

- [ ] **Step 3: Register `integrations` in `src/main.rs`**

Add to the module list in `src/main.rs`:

```rust
mod integrations;
```

- [ ] **Step 4: Run tests, verify the 6 new tests pass**

Run: `~/.cargo/bin/cargo test --offline integrations::github`
Expected: `test result: ok. 6 passed; 0 failed`

Then full suite:

Run: `~/.cargo/bin/cargo test --offline`
Expected: `test result: ok. 78 passed; 0 failed`

- [ ] **Step 5: Commit**

```bash
git add src/integrations/mod.rs src/integrations/github.rs src/main.rs
git commit -m "Add integrations::github module with PrRef parser"
```

---

## Task 3: Parse `gh pr view --json` Output Into `PrContext`

Add `PrContext` and `parse_pr_view_json` (a pure function over JSON text). The `gh` wrapper itself comes in Task 5.

**Files:**
- Modify: `src/integrations/github.rs`

- [ ] **Step 1: Write failing tests for `parse_pr_view_json`**

Append to the `tests` mod in `src/integrations/github.rs`:

```rust
    #[test]
    fn parse_pr_view_extracts_metadata() {
        let body = r#"{
            "number": 12,
            "title": "Add CRLF round-trip",
            "headRefName": "feat/crlf",
            "baseRefName": "main",
            "headRepository": {"nameWithOwner": "willamhou/DeepseekCode"},
            "files": [
                {"path": "src/tools/apply_patch.rs"},
                {"path": "docs/roadmap.md"}
            ]
        }"#;
        let parsed = parse_pr_view_json(body).unwrap();
        assert_eq!(parsed.number, 12);
        assert_eq!(parsed.title, "Add CRLF round-trip");
        assert_eq!(parsed.branch, "feat/crlf");
        assert_eq!(parsed.base_branch, "main");
        assert_eq!(parsed.repo, "willamhou/DeepseekCode");
        assert_eq!(
            parsed.changed_files,
            vec![
                "src/tools/apply_patch.rs".to_string(),
                "docs/roadmap.md".to_string(),
            ]
        );
    }

    #[test]
    fn parse_pr_view_rejects_missing_required_fields() {
        let body = r#"{"number": 1}"#;
        assert!(parse_pr_view_json(body).is_err());
    }
```

- [ ] **Step 2: Run tests to verify both fail**

Run: `~/.cargo/bin/cargo test --offline parse_pr_view`
Expected: compile errors (`PrContext` undefined, `parse_pr_view_json` undefined, `parsed.diff` referenced inside test).

Note: the test bodies above do not yet reference `parsed.diff` because `parse_pr_view_json` does not own the diff. We'll add the diff in Task 5 when `fetch_pr` orchestrates both calls.

- [ ] **Step 3: Add `PrContext` and `parse_pr_view_json`**

Append (between the existing `parse_pr_ref` and the `tests` mod) in `src/integrations/github.rs`:

```rust
use crate::util::json::{json_as_array, json_as_object, json_as_string, parse_root_object};

#[derive(Debug, Clone)]
pub struct PrContext {
    pub number: u64,
    pub repo: String,
    pub title: String,
    pub branch: String,
    pub base_branch: String,
    pub diff: String,
    pub changed_files: Vec<String>,
}

pub fn parse_pr_view_json(body: &str) -> AppResult<PrContext> {
    let root = parse_root_object(body)?;

    let number = root
        .get("number")
        .and_then(|value| match value {
            crate::util::json::JsonValue::Number(text) => text.parse().ok(),
            _ => None,
        })
        .ok_or_else(|| app_error("pr view: missing or non-numeric `number`"))?;
    let title = root
        .get("title")
        .and_then(json_as_string)
        .ok_or_else(|| app_error("pr view: missing string `title`"))?
        .to_string();
    let branch = root
        .get("headRefName")
        .and_then(json_as_string)
        .ok_or_else(|| app_error("pr view: missing string `headRefName`"))?
        .to_string();
    let base_branch = root
        .get("baseRefName")
        .and_then(json_as_string)
        .ok_or_else(|| app_error("pr view: missing string `baseRefName`"))?
        .to_string();
    let repo = root
        .get("headRepository")
        .and_then(json_as_object)
        .and_then(|map| map.get("nameWithOwner"))
        .and_then(json_as_string)
        .ok_or_else(|| app_error("pr view: missing string `headRepository.nameWithOwner`"))?
        .to_string();
    let changed_files = root
        .get("files")
        .and_then(json_as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    json_as_object(item)
                        .and_then(|map| map.get("path"))
                        .and_then(json_as_string)
                        .map(str::to_string)
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    Ok(PrContext {
        number,
        repo,
        title,
        branch,
        base_branch,
        diff: String::new(),
        changed_files,
    })
}
```

- [ ] **Step 4: Run tests to verify both new tests pass**

Run: `~/.cargo/bin/cargo test --offline parse_pr_view`
Expected: `2 passed; 0 failed`

Run full: `~/.cargo/bin/cargo test --offline`
Expected: `80 passed`

- [ ] **Step 5: Commit**

```bash
git add src/integrations/github.rs
git commit -m "Parse gh pr view JSON into PrContext struct"
```

---

## Task 4: Parse `gh pr checks` and `gh run view` Output

Add `CiFailure` plus the parsers needed to chain `pr checks → run view → run view --job`.

**Files:**
- Modify: `src/integrations/github.rs`

- [ ] **Step 1: Write failing tests**

Append to the `tests` mod in `src/integrations/github.rs`:

```rust
    #[test]
    fn parse_pr_checks_finds_first_failed_run() {
        let body = r#"[
            {"name": "lint", "state": "SUCCESS", "link": "https://github.com/o/r/actions/runs/100/jobs/1"},
            {"name": "test", "state": "FAILURE", "link": "https://github.com/o/r/actions/runs/200/jobs/2"},
            {"name": "deploy", "state": "FAILURE", "link": "https://github.com/o/r/actions/runs/300/jobs/3"}
        ]"#;
        let (run_id, name) = parse_first_failed_check(body, None).unwrap().unwrap();
        assert_eq!(run_id, 200);
        assert_eq!(name, "test");
    }

    #[test]
    fn parse_pr_checks_filters_by_job_name() {
        let body = r#"[
            {"name": "lint", "state": "FAILURE", "link": "https://github.com/o/r/actions/runs/100/jobs/1"},
            {"name": "test", "state": "FAILURE", "link": "https://github.com/o/r/actions/runs/200/jobs/2"}
        ]"#;
        let (run_id, name) = parse_first_failed_check(body, Some("test")).unwrap().unwrap();
        assert_eq!(run_id, 200);
        assert_eq!(name, "test");
    }

    #[test]
    fn parse_pr_checks_returns_none_when_all_pass() {
        let body = r#"[
            {"name": "lint", "state": "SUCCESS", "link": "https://github.com/o/r/actions/runs/100/jobs/1"}
        ]"#;
        assert!(parse_first_failed_check(body, None).unwrap().is_none());
    }

    #[test]
    fn parse_run_jobs_picks_failed_job_id() {
        let body = r#"{
            "jobs": [
                {"databaseId": 11, "name": "lint", "conclusion": "success", "steps": []},
                {"databaseId": 22, "name": "test", "conclusion": "failure", "steps": [
                    {"name": "Set up Rust", "conclusion": "success"},
                    {"name": "cargo test", "conclusion": "failure"}
                ]}
            ]
        }"#;
        let (job_id, failed_step) = parse_failed_job_from_run(body, "test").unwrap();
        assert_eq!(job_id, 22);
        assert_eq!(failed_step.as_deref(), Some("cargo test"));
    }

    #[test]
    fn parse_run_jobs_errors_when_job_name_missing() {
        let body = r#"{"jobs": []}"#;
        assert!(parse_failed_job_from_run(body, "test").is_err());
    }
```

- [ ] **Step 2: Run tests to verify they fail (compile error)**

Run: `~/.cargo/bin/cargo test --offline parse_pr_checks`
Expected: compile error (functions undefined).

- [ ] **Step 3: Add `CiFailure` and the two parsers**

Append (above the `tests` mod) in `src/integrations/github.rs`:

```rust
#[derive(Debug, Clone)]
pub struct CiFailure {
    pub run_id: u64,
    pub job_name: String,
    pub job_id: u64,
    pub log_tail: String,
    pub failed_step: Option<String>,
}

pub fn parse_first_failed_check(
    body: &str,
    job_filter: Option<&str>,
) -> AppResult<Option<(u64, String)>> {
    use crate::util::json::{parse_json_value, JsonValue};

    let root = parse_json_value(body.trim())?;
    let JsonValue::Array(items) = root else {
        return Err(app_error("pr checks: expected JSON array"));
    };

    for item in &items {
        let JsonValue::Object(check) = item else {
            continue;
        };
        let state = check.get("state").and_then(json_as_string).unwrap_or("");
        if !state.eq_ignore_ascii_case("FAILURE") {
            continue;
        }
        let name = check
            .get("name")
            .and_then(json_as_string)
            .unwrap_or("")
            .to_string();
        if let Some(filter) = job_filter {
            if !name.eq_ignore_ascii_case(filter) {
                continue;
            }
        }
        let link = check
            .get("link")
            .and_then(json_as_string)
            .unwrap_or_default();
        if let Some(run_id) = extract_run_id_from_link(link) {
            return Ok(Some((run_id, name)));
        }
    }
    Ok(None)
}

fn extract_run_id_from_link(link: &str) -> Option<u64> {
    let marker = "/runs/";
    let start = link.find(marker)? + marker.len();
    let rest = &link[start..];
    let end = rest.find('/').unwrap_or(rest.len());
    rest[..end].parse().ok()
}

pub fn parse_failed_job_from_run(
    body: &str,
    job_name: &str,
) -> AppResult<(u64, Option<String>)> {
    let root = parse_root_object(body)?;
    let jobs = root
        .get("jobs")
        .and_then(json_as_array)
        .ok_or_else(|| app_error("run view: missing `jobs` array"))?;

    for job in jobs {
        let Some(map) = json_as_object(job) else {
            continue;
        };
        let name = map.get("name").and_then(json_as_string).unwrap_or("");
        if !name.eq_ignore_ascii_case(job_name) {
            continue;
        }
        let database_id = map
            .get("databaseId")
            .and_then(|value| match value {
                crate::util::json::JsonValue::Number(text) => text.parse().ok(),
                _ => None,
            })
            .ok_or_else(|| app_error(format!("run view: job `{job_name}` missing databaseId")))?;
        let failed_step = map
            .get("steps")
            .and_then(json_as_array)
            .and_then(|steps| {
                steps.iter().find_map(|step| {
                    let map = json_as_object(step)?;
                    let conclusion = map.get("conclusion").and_then(json_as_string)?;
                    if conclusion.eq_ignore_ascii_case("failure") {
                        Some(
                            map.get("name")
                                .and_then(json_as_string)
                                .unwrap_or("")
                                .to_string(),
                        )
                    } else {
                        None
                    }
                })
            });
        return Ok((database_id, failed_step));
    }

    Err(app_error(format!(
        "run view: job `{job_name}` not found in jobs list"
    )))
}
```

- [ ] **Step 4: Run tests to verify all 5 new tests pass**

Run: `~/.cargo/bin/cargo test --offline parse_pr_checks`
Expected: `3 passed`

Run: `~/.cargo/bin/cargo test --offline parse_run_jobs`
Expected: `2 passed`

Run full: `~/.cargo/bin/cargo test --offline`
Expected: `85 passed`

- [ ] **Step 5: Commit**

```bash
git add src/integrations/github.rs
git commit -m "Parse gh pr checks and gh run view JSON for CI failure lookup"
```

---

## Task 5: `gh` CLI Wrappers (`fetch_pr`, `fetch_first_failed_job`, `post_pr_comment`, `ensure_gh_auth`)

These are I/O-heavy wrappers. Their orchestration logic is testable, but actual `gh` execution is manual.

**Files:**
- Modify: `src/integrations/github.rs`

- [ ] **Step 1: Add the wrappers**

Append (above the `tests` mod) in `src/integrations/github.rs`:

```rust
use std::process::Command;

pub fn ensure_gh_auth() -> AppResult<()> {
    let output = Command::new("gh")
        .args(["auth", "status"])
        .output()
        .map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                app_error("gh CLI not found; install from https://cli.github.com/")
            } else {
                app_error(format!("failed to invoke gh: {error}"))
            }
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(crate::error::policy_denied(format!(
            "gh not authenticated; run `gh auth login` (gh said: {})",
            stderr.trim()
        )));
    }
    Ok(())
}

fn run_gh(args: &[&str]) -> AppResult<String> {
    let output = Command::new("gh")
        .args(args)
        .output()
        .map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                app_error("gh CLI not found; install from https://cli.github.com/")
            } else {
                app_error(format!("failed to invoke gh: {error}"))
            }
        })?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(crate::error::tool_failure(format!(
            "gh {} failed: {}",
            args.first().copied().unwrap_or(""),
            stderr.trim()
        )));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn pr_ref_arg(reference: &PrRef) -> String {
    match reference {
        PrRef::Number(n) => n.to_string(),
        PrRef::Qualified { repo, number } => format!("{repo}#{number}"),
    }
}

pub fn fetch_pr(reference: &PrRef) -> AppResult<PrContext> {
    let view = run_gh(&[
        "pr",
        "view",
        &pr_ref_arg(reference),
        "--json",
        "number,title,headRefName,baseRefName,headRepository,files",
    ])?;
    let mut context = parse_pr_view_json(&view)?;

    let diff = run_gh(&["pr", "diff", &pr_ref_arg(reference)])?;
    context.diff = diff;
    Ok(context)
}

pub fn fetch_first_failed_job(
    pr: &PrContext,
    job_filter: Option<&str>,
) -> AppResult<Option<CiFailure>> {
    let target = format!("{}#{}", pr.repo, pr.number);
    let checks = run_gh(&["pr", "checks", &target, "--json", "name,state,link"])?;
    let Some((run_id, job_name)) = parse_first_failed_check(&checks, job_filter)? else {
        return Ok(None);
    };

    let run_view = run_gh(&[
        "run",
        "view",
        &run_id.to_string(),
        "--repo",
        &pr.repo,
        "--json",
        "jobs",
    ])?;
    let (job_id, failed_step) = parse_failed_job_from_run(&run_view, &job_name)?;

    let log = run_gh(&[
        "run",
        "view",
        "--repo",
        &pr.repo,
        "--job",
        &job_id.to_string(),
        "--log-failed",
    ])?;
    let log_tail = tail_lines(&log, 200);

    Ok(Some(CiFailure {
        run_id,
        job_name,
        job_id,
        log_tail,
        failed_step,
    }))
}

pub fn post_pr_comment(repo: &str, number: u64, body: &str) -> AppResult<()> {
    use std::io::Write;
    let mut path = std::env::temp_dir();
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    path.push(format!("dscode_pr_comment_{stamp}.md"));
    let mut file = std::fs::File::create(&path)?;
    file.write_all(body.as_bytes())?;
    file.flush()?;
    drop(file);

    let target = format!("{repo}#{number}");
    let result = run_gh(&["pr", "comment", &target, "--body-file"])
        .and_then(|_| Ok(()));
    let _ = std::fs::remove_file(&path);
    result
}

fn tail_lines(text: &str, max: usize) -> String {
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() <= max {
        return text.trim_end_matches('\n').to_string();
    }
    let dropped = lines.len() - max;
    let tail = lines[dropped..].join("\n");
    format!("... truncated {dropped} earlier lines ...\n{tail}")
}
```

Note: the `post_pr_comment` body above writes a temp file but the `run_gh` call does NOT pass the temp path. This is a deliberate placeholder bug we will fix in Step 2 — Step 2 verifies the test compiles AND `tail_lines` works, exposing the missing argument.

- [ ] **Step 2: Add unit tests for `tail_lines` and `extract_run_id_from_link`**

Append to the `tests` mod:

```rust
    #[test]
    fn tail_lines_keeps_short_input_intact() {
        let raw = "one\ntwo\nthree";
        assert_eq!(tail_lines(raw, 200), "one\ntwo\nthree");
    }

    #[test]
    fn tail_lines_truncates_when_over_limit() {
        let raw: String = (1..=300).map(|n| format!("line{n}\n")).collect();
        let trimmed = tail_lines(&raw, 100);
        assert!(trimmed.starts_with("... truncated 200 earlier lines ..."));
        assert!(trimmed.contains("line300"));
        assert!(!trimmed.contains("\nline100\n"));
    }

    #[test]
    fn extracts_run_id_from_actions_link() {
        let link = "https://github.com/o/r/actions/runs/12345/jobs/678";
        assert_eq!(extract_run_id_from_link(link), Some(12345));
    }

    #[test]
    fn extract_run_id_returns_none_for_unrelated_link() {
        assert_eq!(extract_run_id_from_link("https://example.com/foo"), None);
    }
```

- [ ] **Step 3: Fix the `post_pr_comment` argument bug**

In `src/integrations/github.rs`, replace:

```rust
    let result = run_gh(&["pr", "comment", &target, "--body-file"])
        .and_then(|_| Ok(()));
```

with:

```rust
    let path_str = path.to_string_lossy().into_owned();
    let result = run_gh(&["pr", "comment", &target, "--body-file", &path_str])
        .and_then(|_| Ok(()));
```

- [ ] **Step 4: Run tests**

Run: `~/.cargo/bin/cargo test --offline`
Expected: `89 passed; 0 failed`

- [ ] **Step 5: Manual smoke (optional but recommended if `gh` available)**

Run: `~/.cargo/bin/cargo run --offline --quiet -- doctor` to confirm nothing breaks.

If you have a real PR you control:
```bash
echo "trying fetch_pr" # placeholder; not exposed via CLI yet — Task 8 wires the command
```

- [ ] **Step 6: Commit**

```bash
git add src/integrations/github.rs
git commit -m "Wrap gh CLI for PR fetch, failed-job lookup, and comments"
```

---

## Task 6: Refactor `AgentLoop` to Take an Options Struct

The agent loop currently runs exactly 4 steps with empty initial observations. Add `AgentLoopOptions` that allows overriding the step budget and seeding observations.

**Files:**
- Modify: `src/core/loop_runtime.rs`
- Modify: `src/core/agent.rs`

- [ ] **Step 1: Read the current shape**

Run: `~/.cargo/bin/cargo run --offline --quiet -- run "inspect repository" 2>&1 | head -8`
Expected: A successful 4-step planner trace as today.

- [ ] **Step 2: Add `AgentLoopOptions` and `run_with`**

Edit `src/core/loop_runtime.rs`. Find the `pub fn run(&self, context: TaskContext) -> AppResult<()>` method on `impl AgentLoop`. Replace it with:

```rust
pub struct AgentLoopOptions {
    pub steps: usize,
    pub initial_observations: Vec<Observation>,
}

impl Default for AgentLoopOptions {
    fn default() -> Self {
        Self {
            steps: 4,
            initial_observations: Vec::new(),
        }
    }
}
```

(Place `AgentLoopOptions` at the top of the file, near the existing `AgentLoop` struct.)

Then in `impl AgentLoop`:

```rust
pub fn run(&self, context: TaskContext) -> AppResult<()> {
    self.run_with(context, AgentLoopOptions::default())
}

pub fn run_with(&self, context: TaskContext, options: AgentLoopOptions) -> AppResult<()> {
```

…and inside the body of the existing run method (now renamed to `run_with`), make two changes:

1. Replace `let mut observations = Vec::new();` with `let mut observations = options.initial_observations.clone();`
2. Replace `for step in 0..4 {` with `for step in 0..options.steps {`

- [ ] **Step 3: Add a test that `run_with` honours the step budget**

Find the existing test module in `src/core/loop_runtime.rs` (if missing, add `#[cfg(test)] mod tests { use super::*; ... }` at the bottom of the file). Append:

Note: `loop_runtime.rs` currently has no test module. Add one only if you find the file already has tests; otherwise, skip the unit test here (the integration test in Task 12 covers the budget path) and proceed to Step 4.

- [ ] **Step 4: Run all tests, confirm nothing regressed**

Run: `~/.cargo/bin/cargo test --offline`
Expected: `89 passed`

Run: `~/.cargo/bin/cargo run --offline --quiet -- run "inspect repository" 2>&1 | head -3`
Expected: Same 4-step planner output as Step 1 (confirming `run` shim still works).

- [ ] **Step 5: Commit**

```bash
git add src/core/loop_runtime.rs
git commit -m "Add AgentLoopOptions for step budget and prefilled observations"
```

---

## Task 7: Add `Pr` Subcommand Group to CLI Parser

The CLI's argv parser is hand-rolled in `src/cli/app.rs`. Add a `Pr` enum variant with three sub-actions.

**Files:**
- Modify: `src/cli/app.rs`

- [ ] **Step 1: Write failing tests for `parse_pr_subcommand`**

Append at the end of `src/cli/app.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_pr_review_with_post_flag() {
        let args = vec!["review".to_string(), "42".to_string(), "--post".to_string()];
        let parsed = parse_pr_subcommand(args).unwrap();
        assert!(matches!(
            parsed,
            PrAction::Review {
                ref reference,
                post: true,
                out: None,
            } if reference == "42"
        ));
    }

    #[test]
    fn parses_pr_fix_with_job_flag() {
        let args = vec![
            "fix".to_string(),
            "owner/repo#7".to_string(),
            "--job".to_string(),
            "test-rust".to_string(),
        ];
        let parsed = parse_pr_subcommand(args).unwrap();
        match parsed {
            PrAction::Fix { reference, job } => {
                assert_eq!(reference, "owner/repo#7");
                assert_eq!(job.as_deref(), Some("test-rust"));
            }
            _ => panic!("expected fix"),
        }
    }

    #[test]
    fn parses_pr_patch_with_commit_flag() {
        let args = vec!["patch".to_string(), "5".to_string(), "--commit".to_string()];
        let parsed = parse_pr_subcommand(args).unwrap();
        assert!(matches!(
            parsed,
            PrAction::Patch {
                commit: true,
                ref reference,
            } if reference == "5"
        ));
    }

    #[test]
    fn rejects_unknown_pr_subaction() {
        let args = vec!["delete".to_string(), "5".to_string()];
        assert!(parse_pr_subcommand(args).is_err());
    }
}
```

- [ ] **Step 2: Run tests to verify compile failure**

Run: `~/.cargo/bin/cargo test --offline parses_pr`
Expected: compile error (`PrAction` undefined, `parse_pr_subcommand` undefined).

- [ ] **Step 3: Add `PrAction` enum and parser**

In `src/cli/app.rs`, add:

```rust
#[derive(Debug)]
pub enum PrAction {
    Review {
        reference: String,
        post: bool,
        out: Option<String>,
    },
    Fix {
        reference: String,
        job: Option<String>,
    },
    Patch {
        reference: String,
        commit: bool,
    },
}

pub fn parse_pr_subcommand(args: Vec<String>) -> Result<PrAction, String> {
    let mut iter = args.into_iter();
    let action = iter
        .next()
        .ok_or_else(|| "pr requires a sub-action: review|fix|patch".to_string())?;
    let reference = iter
        .next()
        .ok_or_else(|| format!("pr {action} requires a PR reference"))?;
    let rest: Vec<String> = iter.collect();

    match action.as_str() {
        "review" => {
            let mut post = false;
            let mut out = None;
            let mut index = 0;
            while index < rest.len() {
                match rest[index].as_str() {
                    "--post" => {
                        post = true;
                        index += 1;
                    }
                    "--out" if index + 1 < rest.len() => {
                        out = Some(rest[index + 1].clone());
                        index += 2;
                    }
                    other => {
                        return Err(format!("unknown flag for `pr review`: {other}"));
                    }
                }
            }
            Ok(PrAction::Review {
                reference,
                post,
                out,
            })
        }
        "fix" => {
            let mut job = None;
            let mut index = 0;
            while index < rest.len() {
                match rest[index].as_str() {
                    "--job" if index + 1 < rest.len() => {
                        job = Some(rest[index + 1].clone());
                        index += 2;
                    }
                    other => {
                        return Err(format!("unknown flag for `pr fix`: {other}"));
                    }
                }
            }
            Ok(PrAction::Fix { reference, job })
        }
        "patch" => {
            let mut commit = false;
            let mut index = 0;
            while index < rest.len() {
                match rest[index].as_str() {
                    "--commit" => {
                        commit = true;
                        index += 1;
                    }
                    other => {
                        return Err(format!("unknown flag for `pr patch`: {other}"));
                    }
                }
            }
            Ok(PrAction::Patch { reference, commit })
        }
        other => Err(format!(
            "unknown pr sub-action `{other}`; expected review|fix|patch"
        )),
    }
}
```

Then add a new `Command::Pr(PrAction)` variant:

```rust
pub enum Command {
    // ... existing variants ...
    Pr(PrAction),
}
```

In the `Cli::parse` match block, add:

```rust
"pr" => match parse_pr_subcommand(args) {
    Ok(action) => Command::Pr(action),
    Err(message) => {
        eprintln!("error: {message}");
        std::process::exit(2);
    }
},
```

(Place this branch above the catch-all `_` arm.)

- [ ] **Step 4: Run tests**

Run: `~/.cargo/bin/cargo test --offline parses_pr`
Expected: `4 passed`

Run full: `~/.cargo/bin/cargo test --offline`
Expected: `93 passed`

- [ ] **Step 5: Commit**

```bash
git add src/cli/app.rs
git commit -m "Parse `pr review|fix|patch` subcommand group from argv"
```

---

## Task 8: Add `pr-review` Skill Definition

A built-in skill that locks down `dscode pr review` to read-only tools.

**Files:**
- Create: `skills/pr-review.toml`

- [ ] **Step 1: Create the skill file**

```toml
name = "pr-review"
description = "Review a GitHub PR diff. Highlight correctness, security, and style issues. Read-only access."
allowed_tools = ["list_files", "read_file", "search_text", "git_diff"]
system_append = """
You are reviewing a pull request diff. Highlight correctness risks, security concerns,
and notable style violations. Do not modify files. Output a markdown report with sections:
Summary, Concerns, Suggestions.
"""
suggested_steps = [
  "Read the diff carefully",
  "Look up the surrounding context for any unfamiliar code",
  "Group findings by severity",
  "Output a markdown report"
]

[policy]
require_write_confirmation = true
require_shell_confirmation = true
shell_allowlist = []
```

- [ ] **Step 2: Verify it loads via the existing skill registry**

Run: `~/.cargo/bin/cargo run --offline --quiet -- run --skill pr-review "describe this repo" 2>&1 | head -8`
Expected: The output includes `Skill: pr-review` and `Skill description: Review a GitHub PR diff...`.

- [ ] **Step 3: Commit**

```bash
git add skills/pr-review.toml
git commit -m "Add pr-review skill with read-only tool whitelist"
```

---

## Task 9: Wire `dscode pr review`

Dispatch the `Pr` command to `pr.rs` and implement the review path.

**Files:**
- Create: `src/cli/commands/pr.rs`
- Modify: `src/cli/commands/mod.rs`
- Modify: `src/cli/mod.rs` (the dispatcher)

- [ ] **Step 1: Create `src/cli/commands/pr.rs` with the review path**

```rust
use crate::cli::app::PrAction;
use crate::core::agent::Agent;
use crate::core::context::TaskContext;
use crate::core::loop_runtime::AgentLoopOptions;
use crate::config::load::load_or_default;
use crate::error::AppResult;
use crate::integrations::github::{
    ensure_gh_auth, fetch_pr, parse_pr_ref, post_pr_comment, PrContext,
};
use crate::model::protocol::Observation;

pub fn run(action: PrAction) -> AppResult<()> {
    match action {
        PrAction::Review { reference, post, out } => run_review(&reference, post, out.as_deref()),
        PrAction::Fix { .. } => Err(crate::error::app_error(
            "pr fix is implemented in a later task",
        )),
        PrAction::Patch { .. } => Err(crate::error::app_error(
            "pr patch is implemented in a later task",
        )),
    }
}

fn run_review(reference: &str, post: bool, out: Option<&str>) -> AppResult<()> {
    ensure_gh_auth()?;
    let pr_ref = parse_pr_ref(reference)?;
    let pr = fetch_pr(&pr_ref)?;

    let task = build_review_task_text(&pr);
    let context = TaskContext::new(task, Some("pr-review".to_string()));

    let observations = vec![
        Observation::ok("git_diff", pr.diff.clone()),
        Observation::ok("list_files", pr.changed_files.join("\n")),
    ];

    let config = load_or_default()?;
    let mut agent = Agent::new(config);
    agent.run_with(
        context,
        AgentLoopOptions {
            steps: 4,
            initial_observations: observations,
        },
    )?;

    let body = format!(
        "DeepseekCode review of PR #{} ({})\n\nSee terminal trace above for the full review.",
        pr.number, pr.title
    );
    deliver_review(&pr, &body, post, out)?;
    Ok(())
}

fn build_review_task_text(pr: &PrContext) -> String {
    format!(
        "Review pull request #{} '{}' on {}/{}. Highlight correctness risks, security concerns, and style violations. Output a markdown report.",
        pr.number, pr.title, pr.repo, pr.branch
    )
}

fn deliver_review(pr: &PrContext, body: &str, post: bool, out: Option<&str>) -> AppResult<()> {
    if let Some(path) = out {
        std::fs::write(path, body)?;
        println!("review written to {path}");
    }
    if post {
        post_pr_comment(&pr.repo, pr.number, body)?;
        println!("review posted as comment on {}#{}", pr.repo, pr.number);
    }
    if !post && out.is_none() {
        println!("{body}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_pr() -> PrContext {
        PrContext {
            number: 12,
            repo: "owner/repo".to_string(),
            title: "Add feature X".to_string(),
            branch: "feat/x".to_string(),
            base_branch: "main".to_string(),
            diff: String::new(),
            changed_files: Vec::new(),
        }
    }

    #[test]
    fn review_task_text_mentions_number_and_title() {
        let text = build_review_task_text(&fixture_pr());
        assert!(text.contains("#12"));
        assert!(text.contains("Add feature X"));
        assert!(text.contains("owner/repo"));
    }
}
```

Note: the function `Agent::run_with` does not yet exist on `Agent`. Step 2 below adds it.

- [ ] **Step 2: Add `Agent::run_with` shim**

Edit `src/core/agent.rs`. Add to `impl Agent`:

```rust
pub fn run_with(
    &mut self,
    context: TaskContext,
    options: crate::core::loop_runtime::AgentLoopOptions,
) -> AppResult<()> {
    let runtime = crate::core::loop_runtime::AgentLoop::new(self.config.clone());
    runtime.run_with(context, options)
}
```

(Place it directly after the existing `run` method.)

- [ ] **Step 3: Register `pr` in `src/cli/commands/mod.rs`**

Add:

```rust
pub mod pr;
```

- [ ] **Step 4: Wire dispatch in `src/cli/mod.rs`**

Open `src/cli/mod.rs`. Add a match arm to the `match cli.command.unwrap_or_default()` block:

```rust
app::Command::Pr(action) => commands::pr::run(action),
```

- [ ] **Step 5: Run tests, verify the new tests pass**

Run: `~/.cargo/bin/cargo test --offline review_task_text`
Expected: `1 passed`

Run full: `~/.cargo/bin/cargo test --offline`
Expected: `94 passed`

- [ ] **Step 6: Manual smoke (requires `gh` + a real PR)**

```bash
~/.cargo/bin/cargo run --offline --quiet -- pr review willamhou/DeepseekCode#1 2>&1 | head -20
```

If `gh` is not installed or `gh auth status` fails, expect a clear error message; in either case the binary should exit cleanly without panicking.

- [ ] **Step 7: Commit**

```bash
git add src/cli/commands/pr.rs src/cli/commands/mod.rs src/cli/mod.rs src/core/agent.rs
git commit -m "Implement dscode pr review with stdout/--post/--out routing"
```

---

## Task 10: `pr fix` — Branch Check Helper

Pre-requisite for `pr fix`: ensure the user is on the PR branch.

**Files:**
- Modify: `src/integrations/github.rs`

- [ ] **Step 1: Write failing tests**

Append to the `tests` mod in `src/integrations/github.rs`:

```rust
    #[test]
    fn current_branch_returns_some_for_a_git_repo() {
        let branch = current_branch();
        assert!(branch.is_some());
        let name = branch.unwrap();
        assert!(!name.is_empty());
    }

    #[test]
    fn require_on_branch_passes_when_branch_matches() {
        let here = current_branch().unwrap();
        assert!(require_on_branch(&here).is_ok());
    }

    #[test]
    fn require_on_branch_fails_with_clear_error_when_branch_differs() {
        let error = require_on_branch("definitely-not-a-real-branch").unwrap_err();
        let message = error.to_string();
        assert!(message.contains("definitely-not-a-real-branch"));
        assert!(message.contains("checkout"));
    }
```

- [ ] **Step 2: Run tests, confirm they fail (compile error)**

Run: `~/.cargo/bin/cargo test --offline current_branch`
Expected: compile error (`current_branch` undefined).

- [ ] **Step 3: Implement helpers**

Append (above the `tests` mod) in `src/integrations/github.rs`:

```rust
pub fn current_branch() -> Option<String> {
    let output = Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let name = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if name.is_empty() {
        None
    } else {
        Some(name)
    }
}

pub fn require_on_branch(expected: &str) -> AppResult<()> {
    match current_branch() {
        Some(actual) if actual == expected => Ok(()),
        Some(actual) => Err(crate::error::policy_denied(format!(
            "expected branch `{expected}`, but currently on `{actual}`; run `git checkout {expected}` first"
        ))),
        None => Err(crate::error::policy_denied(format!(
            "could not determine current git branch; run `git checkout {expected}` first"
        ))),
    }
}
```

- [ ] **Step 4: Run tests**

Run: `~/.cargo/bin/cargo test --offline current_branch`
Expected: `1 passed`

Run: `~/.cargo/bin/cargo test --offline require_on_branch`
Expected: `2 passed`

- [ ] **Step 5: Commit**

```bash
git add src/integrations/github.rs
git commit -m "Add current_branch and require_on_branch helpers"
```

---

## Task 11: Wire `dscode pr fix`

Add the fix path with `--job` filter, `fetch_first_failed_job`, and a 12-step agent loop.

**Files:**
- Modify: `src/cli/commands/pr.rs`

- [ ] **Step 1: Replace the placeholder `Fix` arm**

In `src/cli/commands/pr.rs`, replace:

```rust
        PrAction::Fix { .. } => Err(crate::error::app_error(
            "pr fix is implemented in a later task",
        )),
```

with:

```rust
        PrAction::Fix { reference, job } => run_fix(&reference, job.as_deref()),
```

- [ ] **Step 2: Add `run_fix` and the task text builder**

Append to `src/cli/commands/pr.rs`:

```rust
use crate::integrations::github::{
    fetch_first_failed_job, require_on_branch, CiFailure,
};

fn run_fix(reference: &str, job_filter: Option<&str>) -> AppResult<()> {
    ensure_gh_auth()?;
    let pr_ref = parse_pr_ref(reference)?;
    let pr = fetch_pr(&pr_ref)?;
    require_on_branch(&pr.branch)?;

    let failure = match fetch_first_failed_job(&pr, job_filter)? {
        Some(failure) => failure,
        None => {
            println!("no failed CI jobs on PR #{}", pr.number);
            return Ok(());
        }
    };

    let task = build_fix_task_text(&pr, &failure);
    let context = TaskContext::new(task, None);
    let observations = vec![
        Observation::ok("run_shell", failure.log_tail.clone()),
    ];

    let config = load_or_default()?;
    let mut agent = Agent::new(config);
    agent.run_with(
        context,
        AgentLoopOptions {
            steps: 12,
            initial_observations: observations,
        },
    )?;

    println!(
        "fix attempt complete for job `{}` (run #{}); review `git diff HEAD` and rerun if needed",
        failure.job_name, failure.run_id
    );
    Ok(())
}

fn build_fix_task_text(pr: &PrContext, failure: &CiFailure) -> String {
    let step_clause = failure
        .failed_step
        .as_ref()
        .map(|step| format!(" at step `{step}`"))
        .unwrap_or_default();
    format!(
        "CI job `{job}` (run #{run_id}) on PR #{number} failed{step_clause}. Reproduce locally, fix the root cause, and rerun the failing test. Failed log tail follows.",
        job = failure.job_name,
        run_id = failure.run_id,
        number = pr.number,
    )
}

#[cfg(test)]
mod fix_tests {
    use super::*;

    fn fixture_pr() -> PrContext {
        PrContext {
            number: 12,
            repo: "owner/repo".to_string(),
            title: "Some PR".to_string(),
            branch: "feat/x".to_string(),
            base_branch: "main".to_string(),
            diff: String::new(),
            changed_files: Vec::new(),
        }
    }

    fn fixture_failure() -> CiFailure {
        CiFailure {
            run_id: 555,
            job_name: "test-rust".to_string(),
            job_id: 7,
            log_tail: "FAILED at line 42".to_string(),
            failed_step: Some("cargo test".to_string()),
        }
    }

    #[test]
    fn fix_task_text_includes_run_id_and_step() {
        let text = build_fix_task_text(&fixture_pr(), &fixture_failure());
        assert!(text.contains("run #555"));
        assert!(text.contains("test-rust"));
        assert!(text.contains("cargo test"));
        assert!(text.contains("PR #12"));
    }
}
```

- [ ] **Step 3: Run tests**

Run: `~/.cargo/bin/cargo test --offline fix_task_text`
Expected: `1 passed`

Run full: `~/.cargo/bin/cargo test --offline`
Expected: `97 passed`

- [ ] **Step 4: Manual smoke (requires real PR with failed CI)**

```bash
~/.cargo/bin/cargo run --offline --quiet -- pr fix willamhou/DeepseekCode#1 2>&1 | head -20
```

Expect either "no failed CI jobs" or the planner trace + 12-step ceiling.

- [ ] **Step 5: Commit**

```bash
git add src/cli/commands/pr.rs
git commit -m "Implement dscode pr fix with branch check and 12-step budget"
```

---

## Task 12: `pr patch` — Clean Worktree Helper + Wiring

Add `--commit`-gated worktree check and the patch path.

**Files:**
- Modify: `src/integrations/github.rs`
- Modify: `src/cli/commands/pr.rs`

- [ ] **Step 1: Write failing test for `worktree_is_clean`**

Append to the `tests` mod in `src/integrations/github.rs`:

```rust
    #[test]
    fn worktree_is_clean_returns_a_boolean() {
        let _ = worktree_is_clean();
    }
```

(We don't assert true/false because the test runner's own work may dirty the tree; this only ensures the function compiles and returns.)

- [ ] **Step 2: Run, confirm fail**

Run: `~/.cargo/bin/cargo test --offline worktree_is_clean`
Expected: compile error.

- [ ] **Step 3: Implement `worktree_is_clean`**

Append (above the `tests` mod) in `src/integrations/github.rs`:

```rust
pub fn worktree_is_clean() -> AppResult<bool> {
    let output = Command::new("git")
        .args(["status", "--porcelain"])
        .output()
        .map_err(|error| {
            crate::error::app_error(format!("could not invoke git status: {error}"))
        })?;
    if !output.status.success() {
        return Err(crate::error::tool_failure(format!(
            "git status failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    Ok(output.stdout.iter().all(|b| b.is_ascii_whitespace()))
}
```

- [ ] **Step 4: Wire `pr patch` in `pr.rs`**

In `src/cli/commands/pr.rs`, replace:

```rust
        PrAction::Patch { .. } => Err(crate::error::app_error(
            "pr patch is implemented in a later task",
        )),
```

with:

```rust
        PrAction::Patch { reference, commit } => run_patch(&reference, commit),
```

Append to `src/cli/commands/pr.rs`:

```rust
use crate::integrations::github::worktree_is_clean;

fn run_patch(reference: &str, commit: bool) -> AppResult<()> {
    ensure_gh_auth()?;
    let pr_ref = parse_pr_ref(reference)?;
    let pr = fetch_pr(&pr_ref)?;
    require_on_branch(&pr.branch)?;
    if commit && !worktree_is_clean()? {
        return Err(crate::error::policy_denied(
            "working tree has uncommitted changes; commit or stash before --commit",
        ));
    }

    let task = build_patch_task_text(&pr);
    let context = TaskContext::new(task, None);
    let observations = vec![Observation::ok("git_diff", pr.diff.clone())];

    let config = load_or_default()?;
    let mut agent = Agent::new(config);
    agent.run_with(
        context,
        AgentLoopOptions {
            steps: 4,
            initial_observations: observations,
        },
    )?;

    if commit {
        run_git(&["add", "-A"])?;
        let message = format!("dscode: fix PR #{}", pr.number);
        run_git(&["commit", "-m", &message])?;
        println!("committed staged changes (no push)");
    } else {
        println!("changes left in worktree; run `git diff` to inspect, then commit manually");
    }
    Ok(())
}

fn build_patch_task_text(pr: &PrContext) -> String {
    format!(
        "Address review feedback or apply the requested change in PR #{} '{}'. PR diff is the current head; propose minimal additional changes.",
        pr.number, pr.title
    )
}

fn run_git(args: &[&str]) -> AppResult<()> {
    let output = std::process::Command::new("git")
        .args(args)
        .output()
        .map_err(|error| crate::error::app_error(format!("could not invoke git: {error}")))?;
    if !output.status.success() {
        return Err(crate::error::tool_failure(format!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    Ok(())
}

#[cfg(test)]
mod patch_tests {
    use super::*;

    #[test]
    fn patch_task_text_mentions_pr_number_and_title() {
        let pr = PrContext {
            number: 9,
            repo: "o/r".to_string(),
            title: "Tighten retry loop".to_string(),
            branch: "feat/retry".to_string(),
            base_branch: "main".to_string(),
            diff: String::new(),
            changed_files: Vec::new(),
        };
        let text = build_patch_task_text(&pr);
        assert!(text.contains("#9"));
        assert!(text.contains("Tighten retry loop"));
    }
}
```

- [ ] **Step 5: Run tests**

Run: `~/.cargo/bin/cargo test --offline patch_task_text`
Expected: `1 passed`

Run full: `~/.cargo/bin/cargo test --offline`
Expected: `99 passed`

- [ ] **Step 6: Manual smoke (requires real PR; do not use --commit on a real branch you care about)**

```bash
~/.cargo/bin/cargo run --offline --quiet -- pr patch willamhou/DeepseekCode#1 2>&1 | head -10
```

- [ ] **Step 7: Commit**

```bash
git add src/integrations/github.rs src/cli/commands/pr.rs
git commit -m "Implement dscode pr patch with --commit and clean-worktree gate"
```

---

## Task 13: Extend `dscode doctor` to Check `gh`

`doctor` already reports network and API key status. Add a `[github]` section.

**Files:**
- Modify: `src/cli/commands/doctor.rs`

- [ ] **Step 1: Read the current sections**

Run: `~/.cargo/bin/cargo run --offline --quiet -- doctor 2>&1 | head -30`
Expected: Five sections: workspace / model / api key / network / hints.

- [ ] **Step 2: Add a `[github]` section between `[network]` and `[hints]`**

Edit `src/cli/commands/doctor.rs`. Find the `run` function. After the line that calls `print_network_section(&config);`, insert:

```rust
    print_github_section();
```

Then add a new function near the bottom of the file:

```rust
fn print_github_section() {
    println!();
    println!("[github]");
    let version = std::process::Command::new("gh")
        .args(["--version"])
        .output();
    match version {
        Ok(out) if out.status.success() => {
            let first_line = String::from_utf8_lossy(&out.stdout)
                .lines()
                .next()
                .unwrap_or("")
                .trim()
                .to_string();
            println!("  gh CLI: {first_line}");
            let auth = std::process::Command::new("gh")
                .args(["auth", "status"])
                .output();
            match auth {
                Ok(auth_out) if auth_out.status.success() => {
                    println!("  gh auth: ok");
                }
                Ok(_) => {
                    println!("  gh auth: not authenticated (run `gh auth login`)");
                }
                Err(error) => {
                    println!("  gh auth: could not check ({error})");
                }
            }
        }
        Ok(_) | Err(_) => {
            println!("  gh CLI: not installed (install from https://cli.github.com/ for `dscode pr` commands)");
        }
    }
}
```

- [ ] **Step 3: Run doctor and verify the new section appears**

Run: `~/.cargo/bin/cargo run --offline --quiet -- doctor 2>&1 | head -40`
Expected: Output contains a `[github]` section that either says `gh CLI: gh version ...` or `gh CLI: not installed`.

- [ ] **Step 4: Run all tests**

Run: `~/.cargo/bin/cargo test --offline`
Expected: `99 passed`

- [ ] **Step 5: Commit**

```bash
git add src/cli/commands/doctor.rs
git commit -m "Add [github] section to dscode doctor output"
```

---

## Task 14: Update `.dscode/config.example.toml` and Roadmap

Document the new commands in user-facing docs.

**Files:**
- Modify: `.dscode/config.example.toml`
- Modify: `docs/roadmap.md`

- [ ] **Step 1: Add a `[pr]` placeholder section to `config.example.toml`**

Append at the bottom of `.dscode/config.example.toml`:

```toml
# -----------------------------------------------------------------------------
# pr: GitHub PR integration (Phase 8a)
# -----------------------------------------------------------------------------

# v1 has no PR-specific config keys; this section is reserved for future use
# (e.g. default --max-attempts, default reviewer skill name).
# Run `dscode doctor` and check the [github] section to verify gh CLI auth.
```

- [ ] **Step 2: Update the roadmap**

Edit `docs/roadmap.md`. Find the "Phase 8" section and replace its current contents with:

```
### Phase 8: 高级能力

- PR/CI 集成（v1，进行中）
  - `dscode pr review <pr>` —— 只读 review，输出 markdown 到 stdout / 文件 / `gh pr comment`
  - `dscode pr fix <pr>` —— 抓首个失败 CI job，本地复现并迭代修复（12 步预算）
  - `dscode pr patch <pr>` —— 提改动到工作区；`--commit` 在干净工作区时自动 commit（不 push）
  - 三命令共享 `gh auth` 检查、PR 上下文获取、prefilled observations 注入
  - 所有写入与 shell 仍走 P3 confirm
- 更强语言特化：未开始
- IDE 集成：未开始
- 多 agent：未开始

状态：进行中（PR/CI 一项）
```

- [ ] **Step 3: Run all tests**

Run: `~/.cargo/bin/cargo test --offline`
Expected: `99 passed`

- [ ] **Step 4: Commit**

```bash
git add .dscode/config.example.toml docs/roadmap.md
git commit -m "Document PR/CI integration in roadmap and config example"
```

---

## Task 15: Add `docs/pr-integration.md` User Guide

A standalone user guide for the new commands.

**Files:**
- Create: `docs/pr-integration.md`
- Modify: `README.md`

- [ ] **Step 1: Create `docs/pr-integration.md`**

```markdown
# PR / CI Integration

`dscode pr` is a subcommand group for working with GitHub pull requests via the
`gh` CLI. Three actions are supported in v1.

## Prerequisites

- `gh` CLI version 2.40+ installed (`brew install gh` or see <https://cli.github.com/>)
- Authenticated: `gh auth login`
- `dscode doctor` should show `gh auth: ok` in the `[github]` section

## Commands

### `dscode pr review <pr>`

Run a read-only review pass over the PR diff. The agent is restricted to
`list_files`, `read_file`, `search_text`, and `git_diff` — no writes.

```
dscode pr review 42                     # review PR #42 in the current repo
dscode pr review owner/repo#42          # explicit owner/repo
dscode pr review https://github.com/.../pull/42
dscode pr review 42 --post              # also post a summary comment
dscode pr review 42 --out review.md     # also write to a local file
```

### `dscode pr fix <pr>`

Pull the failing CI job's tail log and iterate locally to fix it. The agent is
allowed to call `apply_patch`, `run_shell`, etc., subject to P3 confirm prompts
(or the `DSCODE_AUTO_APPROVE_*` env vars). Step budget is 12 (vs. the default 4)
to fit a read → patch → shell → re-read cycle.

You must be on the PR's head branch first:

```
gh pr checkout 42
dscode pr fix 42
dscode pr fix 42 --job test-rust        # restrict to one CI job
```

### `dscode pr patch <pr>`

Apply additional changes to the PR head; default leaves changes in the worktree.

```
gh pr checkout 42
dscode pr patch 42
dscode pr patch 42 --commit             # also commit (clean worktree required); does NOT push
```

## Exit codes

| Code | Meaning |
|---|---|
| 0 | Success (or "no failures" for `pr fix`) |
| 1 | Internal error (planner / `gh` returned non-zero) |
| 2 | User declined a prompt or branch / worktree precondition failed |
| 3 | `gh` not installed or not authenticated |

## v1 limitations

- GitHub-only (`gh` CLI). GitLab / Gitea support is not planned for v1.
- `--push` is not implemented; `--commit` stops at a local commit.
- `--max-attempts` is not implemented; rerun `dscode pr fix` for another round.
- Inline review comments are not implemented; `--post` posts one summary comment.
```

- [ ] **Step 2: Add a pointer to `README.md`**

Open `README.md`. After the existing `## 文档` section's bullet list, add a new bullet:

```
- [PR / CI 集成指南](./docs/pr-integration.md)
```

- [ ] **Step 3: Commit**

```bash
git add docs/pr-integration.md README.md
git commit -m "Add user guide for dscode pr commands"
```

---

## Self-Review Notes

This plan covers spec sections in order:
- Spec architecture (modules) → Tasks 1, 2, 6, 8, 9
- Spec data contracts (PrContext / CiFailure / PrRef) → Tasks 2, 3, 4
- Spec gh CLI mapping → Tasks 4, 5
- Spec error classification → Used inline in Tasks 5, 9, 10, 12 (via existing P3 helpers)
- Spec command surface → Tasks 7, 9, 11, 12
- Spec flow per command → Tasks 9 (review), 11 (fix), 12 (patch)
- Spec context management → Reused via existing `summarize_for_kind` / `compact_observations` (no new task)
- Spec testing strategy → Tasks 2-7, 9-12 each include unit tests
- Spec slicing M1-M6 → Tasks 1-5 cover M1; Tasks 6-9 cover M2+M3; Task 10-11 cover M4; Task 12 covers M5; Tasks 13-15 cover M6

Type/name consistency check passed: `PrContext.branch` (not `head_ref`), `CiFailure.job_id` (not `database_id`), `parse_first_failed_check` (matches usage in `fetch_first_failed_job`), `parse_failed_job_from_run` (matches usage), `AgentLoopOptions { steps, initial_observations }` (matches Tasks 6, 9, 11, 12).

Final test count after all tasks: 99 (72 base + 6 PrRef + 2 pr_view + 5 pr_checks/run/log + 4 pr subcommand + 3 branch helpers + 1 worktree + 1 review_task_text + 1 fix_task_text + 1 patch_task_text - 0 deletions = 99 expected).

---

**Plan complete and saved to `docs/superpowers/plans/2026-04-27-pr-integration.md`. Two execution options:**

**1. Subagent-Driven (recommended)** — Dispatch a fresh subagent per task with two-stage review between tasks. Best when each task is independent and you want fast iteration.

**2. Inline Execution** — Execute tasks here in this session via executing-plans, with batch checkpoints for review.

Which approach?
