# DeepSeek-TUI GitHub Context Parity Spec

日期：2026-05-13

对比对象：`Hmbown/DeepSeek-TUI`，`main` HEAD `3382242`

## 背景

DeepSeek-TUI exposes read-only GitHub context tools:
`github_issue_context` and `github_pr_context`, backed by the `gh` CLI. These
let the model inspect issue/PR metadata, comments, reviews, checks, files, and
optionally diffs without shelling out manually.

DeepSeekCode already has local `deepseek pr review|fix|patch` commands and
GitHub parsing helpers, but the default agent tool registry does not expose the
direct model-callable GitHub context tool names.

## 目标

- Add `github_issue_context` as a read-only agent tool.
- Add `github_pr_context` as a read-only agent tool.
- Use `gh` through argv, not shell interpolation.
- Support optional `repo` / `owner/repo` scoping.
- Support `github_pr_context include_diff=true` with bounded diff output.
- Expose both tools through model schemas, MCP tools, and ACP session tools.
- Keep tests hermetic with a fake `gh` executable.

## 非目标

- This slice does not implement `github_comment` or `github_close_issue`.
- This slice does not post, merge, close, checkout, push, or mutate GitHub
  state.
- This slice does not replace the existing `deepseek pr` workflow.

## 验收标准

1. `github_pr_context number=<n>` calls `gh pr view` and summarizes the PR JSON.
2. `github_pr_context include_diff=true` also calls `gh pr diff --patch` and
   includes a bounded diff excerpt.
3. `github_issue_context number=<n>` calls `gh issue view` and summarizes the
   issue JSON.
4. Registry/model/MCP/ACP tool lists include both read-only GitHub context
   tools.
5. Tests do not require network or real GitHub auth.

## 实现结果

- Added `src/tools/github.rs` with read-only `github_issue_context` and
  `github_pr_context` tools backed by the `gh` CLI.
- `github_issue_context` reads `gh issue view` JSON and supports
  `include_comments=false`, `repo` / `repository`, and `max_chars`.
- `github_pr_context` reads `gh pr view` JSON and supports
  `include_diff=true`, `repo` / `repository`, `max_chars`, and
  `diff_max_chars`.
- Registered both tools in the default tool registry, model schemas, MCP
  `tools/list`, and ACP session tool listing.
- Added hermetic fake-`gh` tests so validation does not require network or
  GitHub authentication.

## 验证

- `cargo test github` passed: 25 tests.
- `cargo test default_registry_includes_read_only_git_history_tools` passed.
- `cargo test mcp_tools_list_includes_workspace_and_runtime_tools` passed.
- `cargo test acp_session_tools_list_new_session_is_read_only` passed.
- `cargo fmt --check` passed.
- `git diff --check` passed.
- `cargo test` passed: 922 tests.
- `cargo package --allow-dirty` passed: 270 packaged files.
