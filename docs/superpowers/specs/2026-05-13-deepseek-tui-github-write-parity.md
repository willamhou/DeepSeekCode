# DeepSeek-TUI GitHub Write Parity Spec

日期：2026-05-13

对比对象：`Hmbown/DeepSeek-TUI`，`main` HEAD `3382242`

## 背景

DeepSeek-TUI exposes two approval-gated GitHub mutation tools:
`github_comment` and `github_close_issue`. They let the model post
evidence-backed issue/PR comments and close completed issues without falling
back to shell commands.

DeepSeekCode now has read-only GitHub context tools, but it still lacks these
direct guarded mutation tool names.

## 目标

- Add `github_comment` as an approval-gated agent tool.
- Add `github_close_issue` as an approval-gated agent tool.
- Use `gh` through argv, not shell interpolation.
- Require structured evidence for comments and issue closure.
- Refuse issue closure on a dirty worktree unless `allow_dirty=true`.
- Support optional `repo` / `repository` scoping.
- Expose both tools through model schemas and MCP/ACP only when durable write
  approvals are enabled.
- Keep tests hermetic with dry-run paths or a fake `gh` executable.

## 非目标

- This slice does not merge PRs, push branches, edit labels, or assign users.
- This slice does not bypass the existing write approval policy.
- This slice does not replace the existing `deepseek pr` command workflow.

## 验收标准

1. `github_comment target=issue|pr number=<n> body=<text> evidence=<json>` posts
   through `gh issue comment` or `gh pr comment`.
2. `github_comment dry_run=true` validates inputs without invoking `gh`.
3. `github_close_issue number=<n> acceptance_criteria=<json-array>
   evidence=<json>` validates structured completion evidence.
4. `github_close_issue` refuses dirty worktrees by default and allows override
   with `allow_dirty=true`.
5. Registry/model/MCP/ACP gated tool lists include both write tools under the
   same durable approval contract as other write tools.
6. Tests do not require network or real GitHub auth.

## 实现结果

- Added approval-gated `github_comment` and `github_close_issue` tools in
  `src/tools/github.rs`.
- `github_comment` validates `target`, positive `number`, non-empty `body`, and
  a non-empty evidence JSON object before posting through `gh issue comment` or
  `gh pr comment`.
- `github_close_issue` validates acceptance criteria, structured evidence with
  `files_changed`, `tests_run`, and completed `final_status`, refuses dirty
  worktrees by default, optionally posts a closing comment, and closes through
  `gh issue close --reason completed`.
- Registered both tools in the default registry and model schemas with write
  permission requests.
- Exposed both through MCP/ACP only when durable runtime approvals are enabled.
- Added hermetic fake-`gh`, dry-run, dirty-worktree, permission, and tool-list
  tests.

## 验证

- `cargo test github` passed: 31 tests.
- `cargo test default_registry_includes_read_only_git_history_tools` passed.
- `cargo test permission_request_for_reports_write_shell_and_mcp_prompts`
  passed.
- `cargo test mcp_tools_list_includes_write_tools_only_with_durable_approvals`
  passed.
- `cargo test acp_session_tools_list_new_session_is_read_only` passed.
- `cargo fmt --check` passed.
- `git diff --check` passed.
- `cargo test` passed: 928 tests.
- `cargo package --allow-dirty` passed: 271 packaged files.
