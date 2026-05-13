# DeepSeek-TUI Review Tool Parity Spec

日期：2026-05-13

对比对象：`Hmbown/DeepSeek-TUI`，`main` HEAD `3382242`

## 背景

DeepSeek-TUI exposes an agent-visible `review` tool for structured code reviews
of files, diffs, or pull requests. DeepSeekCode currently has `deepseek pr
review` and MCP `review_code`, but no direct `review` tool on the agent-visible
surface.

## 目标

- Add agent-visible `review`.
- Accept required `target` plus optional `kind`, `base`, `staged`, `cwd`, and
  `max_chars`.
- Support file review for safe relative workspace files.
- Support diff review for `target=diff`, `target=staged`, or `kind=diff`.
- Return structured JSON with `summary`, `issues`, `suggestions`, and
  `overall_assessment`.
- Include deterministic local review signals for obvious risk markers such as
  conflict markers, `unwrap()`, `panic!`, `todo!`, `unimplemented!`, `dbg!`,
  broad `unsafe`, and debug prints.
- Expose the tool in the default registry and DeepSeek model schema.

## 非目标

- This slice does not call a child LLM for semantic review.
- This slice does not fetch remote PRs; agents should use `github_pr_context`
  first when remote PR context is needed.
- This slice does not post review comments.

## 验收标准

1. `review target=<file>` reads a safe relative file and returns structured
   review JSON.
2. `review target=diff` reviews the current working-tree diff.
3. `review target=staged` or `staged=true` reviews staged changes.
4. Obvious markers produce issue records with severity/path/line when available.
5. The default registry exposes `review`.
6. The model schema exposes `review`.

## 实现结果

- Added `ReviewTool` in `src/tools/review.rs`.
- `review` accepts `target`, optional `kind`, `base`, `staged`, `cwd`, and
  `max_chars`.
- File review reads safe relative files under the selected workspace.
- Diff review supports `target=diff`, `target=staged`, `kind=diff`, optional
  `base`, and `staged=true`.
- The tool returns structured JSON with `summary`, `issues`, `suggestions`,
  `overall_assessment`, and source metadata.
- Deterministic marker checks report conflict markers, `unwrap()` / `expect`,
  `panic!`, `todo!`, `unimplemented!`, `dbg!`, debug prints, and broad
  `unsafe` usage with path/line when available.
- Registered `review` in the default agent registry.
- Added the DeepSeek model schema entry.
- Updated runtime docs and the DeepSeek-TUI parity plan. Child-LLM semantic
  review remains a follow-up gap.

## 验证

- `cargo test review`: passed, 11 matching tests.
- `cargo test default_registry_includes_read_only_git_history_tools`: passed.
- `cargo fmt --check`: passed.
- `git diff --check`: passed.
- `cargo test`: passed, 981 tests.
- `cargo package --allow-dirty`: passed, packaged 290 files and verified
  `deepseek_code v0.1.0`.
