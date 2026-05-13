# DeepSeek-TUI Tool Output Retrieval Parity Spec

日期：2026-05-13

对比对象：`Hmbown/DeepSeek-TUI`，`main` HEAD `3382242`

## 背景

DeepSeek-TUI spills oversized successful tool outputs to
`~/.deepseek/tool_outputs/` and exposes `retrieve_tool_result` so the model can
fetch summary, head, tail, line ranges, or query matches later without replaying
the whole output into every turn.

DeepSeekCode currently trims observations and caps some individual tools, but
there is no model-visible retrieval tool and no common spillover pointer for
large generic tool outputs.

## 目标

- Add a read-only `retrieve_tool_result` tool.
- Support DeepSeek-TUI-compatible `ref`, `mode`, `query`, `lines`,
  `start_line`, `end_line`, `line_count`, `max_bytes`, `max_matches`, and
  `context_lines` inputs.
- Store large successful tool outputs under `~/.deepseek/tool_outputs/` using a
  sanitized generated id and replace the inline output with a bounded head plus
  retrieval hints.
- Keep error outputs inline so failures remain immediately visible.
- Expose the tool through model, MCP, and ACP tool lists.

## 非目标

- This slice does not add LLM synthesis of large outputs.
- This slice does not add session-scoped artifact storage.
- This slice does not change per-tool output semantics except for oversized
  successful outputs.

## 验收标准

1. Oversized successful tool summaries are spilled to disk and include a
   `retrieve_tool_result ref=<id>` hint.
2. `retrieve_tool_result` can read by id, `tool_result:<id>`, filename, or
   spillover path under the allowed root.
3. Retrieval supports `summary`, `head`, `tail`, `lines`, and `query` modes with
   bounded output.
4. Path traversal or absolute paths outside the spillover root are rejected.
5. OpenAI/Anthropic/MCP/ACP schemas expose the new read-only tool.

## 实现结果

- `src/tools/tool_output.rs` adds spillover storage plus the
  `retrieve_tool_result` read-only tool.
- `src/core/loop_runtime.rs` now spills oversized successful tool summaries
  before rendering/recording them.
- `src/tools/registry.rs`, `src/model/deepseek.rs`, and
  `src/cli/commands/serve.rs` expose `retrieve_tool_result` to agent, MCP, and
  ACP clients.
- `docs/runtime.md` and the DeepSeek-TUI parity plan document the new behavior.

## 验证

- `cargo test tool_output`
- `cargo test retrieve_tool_result`
- `cargo test default_registry_includes_read_only_git_history_tools`
- `cargo test mcp_tools_list_includes_workspace_and_runtime_tools`
- `cargo test acp_session_tools_list_new_session_is_read_only`
- `cargo fmt --check`
- `git diff --check`
- `cargo test` (902 passed)
- `cargo package --allow-dirty`
