# DeepSeek-TUI Review Behavioral Signals Parity Spec

日期：2026-05-13

对比对象：`Hmbown/DeepSeek-TUI`，`main` HEAD `3382242`

## 背景

DeepSeekCode has an agent-visible `review` tool, but its first slice mostly
reports per-line deterministic markers. DeepSeek-TUI's review workflow is
closer to a semantic review assistant that reasons about behavior, tests, and
project-level risk. A full child-LLM reviewer remains larger work, but local
behavioral signals can close part of that gap without live model dependency.

## 目标

- Add deterministic behavioral review signals for git diffs.
- Flag production/source changes that do not include test changes.
- Flag public API additions or changes that need compatibility and coverage
  review.
- Flag dependency/configuration changes that need release or operational
  review.
- Add a light file-review signal for public API files that do not include local
  tests.

## 非目标

- This slice does not call a child LLM for semantic review.
- This slice does not fetch or post remote PR review comments.
- This slice does not perform language-specific AST parsing.

## 验收标准

1. Diff review reports a test-coverage warning when source files change without
   any test file changes.
2. Diff review reports public API risk for added public declarations.
3. Diff review reports dependency/configuration risk for manifest changes.
4. File review reports a local-test suggestion for public API files without
   local test markers.
5. Existing marker review behavior still works.

## 实现结果

- Added deterministic behavioral review signals to `ReviewTool`.
- Diff review now detects source changes without matching test-file changes.
- Diff review flags added public declarations and manifest/configuration
  changes for compatibility, release, and coverage review.
- File review flags public API files that do not contain local test markers.
- Updated runtime docs, model tool description, and the DeepSeek-TUI parity
  plan. Full child-LLM semantic review remains a follow-up.

## 验证

- `cargo test review_diff_reports_behavioral_risk_signals`: passed.
- `cargo test review_file_reports_public_api_without_local_tests`: passed.
- `cargo test review_diff_reports_added_line_markers`: passed.
- `cargo test review`: passed, 13 tests.
- `cargo test build_tool_specs_include_review`: passed.
- `cargo test`: passed, 1031 tests.
- `cargo fmt --check`: passed.
- `git diff --check`: passed.
- `cargo package --allow-dirty`: passed, packaged 307 files.
