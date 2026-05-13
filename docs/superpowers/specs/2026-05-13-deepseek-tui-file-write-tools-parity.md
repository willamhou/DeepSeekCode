# DeepSeek-TUI File Write Tools Parity Spec

日期：2026-05-13

对比对象：`Hmbown/DeepSeek-TUI`，`main` HEAD `3382242`

## 背景

DeepSeek-TUI exposes `write_file` and `edit_file` as agent-callable file
mutation tools. `write_file` creates or overwrites UTF-8 files and auto-creates
parents. `edit_file` performs exact search/replace in one file and reports the
replacement count.

DeepSeekCode has `apply_patch` as the main agent-visible write tool and has
MCP-only `write_file`, but the direct agent tool names are missing.

## 目标

- Add agent-visible `write_file`.
- Add agent-visible `edit_file`.
- Keep both tools approval-gated through the existing write permission policy.
- Restrict paths to a safe relative path under `cwd` / current workspace.
- Refuse symlink or directory targets.
- `write_file` creates parent directories and reports created vs overwritten.
- `edit_file` rejects identical search/replace, reports not-found searches,
  and replaces all exact occurrences.
- Expose both through model schemas.

## 非目标

- This slice does not change MCP `write_file` behavior.
- This slice does not add binary file editing.
- This slice does not implement multi-hunk patch semantics; `apply_patch`
  remains the preferred multi-file tool.

## 验收标准

1. `write_file path=<p> content=<text>` writes UTF-8 content under the workspace
   and creates parent directories.
2. `write_file` refuses unsafe paths, symlinks, and directories.
3. `edit_file path=<p> search=<old> replace=<new>` replaces all exact matches
   and reports the occurrence count.
4. `edit_file` refuses missing search strings and identical search/replace.
5. Registry and model schemas include both tools.
6. Existing write approval policy requests apply to both tools.

## 实现结果

- Added `src/tools/file_write.rs` with agent-visible `write_file` and
  `edit_file`.
- `write_file` writes UTF-8 content under `cwd` / current workspace,
  auto-creates parents, and refuses unsafe, symlink, or directory targets.
- `edit_file` performs exact all-occurrence replacement, rejects identical
  search/replace, reports missing search strings, and refuses unsafe, symlink,
  or directory targets.
- Registered both tools in the default registry and model schemas.
- Routed both tools through the existing write permission policy; `edit_file`
  is also available through MCP/ACP only when durable approvals are enabled.
- Updated runtime and parity documentation.

## 验证

- `cargo test file_write` passed: 6 tests.
- `cargo test default_registry_includes_read_only_git_history_tools` passed.
- `cargo test permission_request_for_reports_write_shell_and_mcp_prompts`
  passed.
- `cargo test mcp_tools_call_executes_edit_file_after_runtime_approval` passed.
- `cargo test mcp_tools_list_includes_write_tools_only_with_durable_approvals`
  passed.
- `cargo test acp_loaded_session_tools_call_write_file_uses_runtime_approval`
  passed.
- `cargo fmt --check` passed.
- `git diff --check` passed.
- `cargo test` passed: 935 tests.
- `cargo package --allow-dirty` passed: 273 packaged files.
