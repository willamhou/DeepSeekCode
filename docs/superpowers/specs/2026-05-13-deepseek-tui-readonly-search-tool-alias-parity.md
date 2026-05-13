# DeepSeek-TUI Read-Only Search Tool Alias Parity Spec

日期：2026-05-13

对比对象：`Hmbown/DeepSeek-TUI`，`main` HEAD `3382242`

## 背景

DeepSeek-TUI exposes familiar read-only file discovery tools such as
`list_dir`, `grep_files`, and `file_search`. DeepSeekCode already had equivalent
core behavior under `list_files` and `search_text`, but model prompts or migrated
tool calls that use DeepSeek-TUI names still missed the registry/schema surface.

## 目标

- Expose `list_dir` as a DeepSeek-TUI-compatible alias for directory listing.
- Expose `grep_files` as a DeepSeek-TUI-compatible alias for literal text search.
- Add `file_search` for filename/path matching with optional extension filtering.
- Include all three tools in the default registry, model tool schemas, and MCP/ACP
  tool listing/call path.
- Keep existing `list_files` and `search_text` behavior unchanged.

## 非目标

- This slice does not add a full regex engine to `grep_files`.
- This slice does not add ripgrep-style include/exclude/context-line options.
- This slice does not change model offline planning heuristics.

## 验收标准

1. Default registry exposes `list_dir`, `grep_files`, and `file_search`.
2. OpenAI/Anthropic tool schemas contain all three tool names and expected args.
3. `list_dir` maps `path` to the existing list root behavior.
4. `grep_files` maps `pattern`/`path`/`max_results` to literal search.
5. `file_search` finds filenames by exact/substring/simple ordered-character match
   and honors extension filters.
6. MCP/ACP tool listing and call dispatch include all three names.

## 实现结果

- `src/tools/list_files.rs` adds `ListDirTool`.
- `src/tools/search_text.rs` adds `GrepFilesTool`.
- `src/tools/file_search.rs` adds `FileSearchTool`.
- `src/tools/registry.rs`, `src/model/deepseek.rs`, and
  `src/cli/commands/serve.rs` expose the aliases to agents, schemas, MCP, and ACP.
- `docs/runtime.md` and the DeepSeek-TUI parity plan document the expanded
  read-only tool surface.

## 验证

- `/home/willamhou/.cargo/bin/cargo fmt --check`
- `/home/willamhou/.cargo/bin/cargo test file_search`
- `/home/willamhou/.cargo/bin/cargo test list_dir`
- `/home/willamhou/.cargo/bin/cargo test grep_files`
- `/home/willamhou/.cargo/bin/cargo test build_tool_specs_include_readonly_search_aliases`
- `/home/willamhou/.cargo/bin/cargo test default_registry_includes_read_only_git_history_tools`
- `/home/willamhou/.cargo/bin/cargo test mcp_tools_list_includes_workspace_and_runtime_tools`
- `/home/willamhou/.cargo/bin/cargo test acp_session_tools_list_new_session_is_read_only`
- `/home/willamhou/.cargo/bin/cargo test`
- `git diff --check`
- `/home/willamhou/.cargo/bin/cargo package --allow-dirty`
