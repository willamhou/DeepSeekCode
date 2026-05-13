# DeepSeek-TUI Git Status And Project Map Parity Spec

日期：2026-05-13

对比对象：`Hmbown/DeepSeek-TUI`，`main` HEAD `3382242`

## 背景

DeepSeek-TUI exposes read-only `git_status` and `project_map` tools. DeepSeekCode
already had `git_diff` plus git history tools, and directory listing, but lacked
the direct concise status and high-level project map entrypoints that help models
orient before editing.

## 目标

- Add `git_status` as a read-only `git status --porcelain=v1 -b` wrapper.
- Add `project_map` with tree, summary counts, and key-file discovery.
- Expose both tools through the default registry, OpenAI/Anthropic tool schemas,
  MCP `tools/list`/`tools/call`, and ACP session tool listing/calls.
- Keep existing `git_diff`, `list_files`, and `search_text` behavior unchanged.

## 非目标

- This slice does not add DeepSeek-TUI's richer `git_diff` path/cached/unified
  options.
- This slice does not add dependency-backed ignore-file traversal.
- This slice does not implement large-output retrieval for project maps.

## 验收标准

1. Default registry exposes `git_status` and `project_map`.
2. OpenAI/Anthropic schemas include both tools and their expected arguments.
3. `git_status` reports branch/status porcelain output and supports optional
   `path`.
4. `project_map` reports a tree, directory/file counts, and common key files.
5. MCP/ACP tool listing and call dispatch include both names.

## 实现结果

- `src/tools/git_diff.rs` adds `GitStatusTool`.
- `src/tools/project_map.rs` adds `ProjectMapTool`.
- `src/tools/registry.rs`, `src/model/deepseek.rs`, and
  `src/cli/commands/serve.rs` expose both tools to agents, schemas, MCP, and
  ACP.
- `docs/runtime.md` and the DeepSeek-TUI parity plan document the expanded
  read-only orientation surface.

## 验证

- `/home/willamhou/.cargo/bin/cargo fmt --check`
- `/home/willamhou/.cargo/bin/cargo test git_status`
- `/home/willamhou/.cargo/bin/cargo test project_map`
- `/home/willamhou/.cargo/bin/cargo test default_registry_includes_read_only_git_history_tools`
- `/home/willamhou/.cargo/bin/cargo test mcp_tools_list_includes_workspace_and_runtime_tools`
- `/home/willamhou/.cargo/bin/cargo test acp_session_tools_list_new_session_is_read_only`
- `/home/willamhou/.cargo/bin/cargo test`
- `git diff --check`
- `/home/willamhou/.cargo/bin/cargo package --allow-dirty`
