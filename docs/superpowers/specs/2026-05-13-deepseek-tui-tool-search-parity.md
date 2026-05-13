# DeepSeek-TUI Tool Search Parity Spec

日期：2026-05-13

对比对象：`Hmbown/DeepSeek-TUI`，`main` HEAD `3382242`

## 背景

DeepSeek-TUI exposes `tool_search_tool_regex` and `tool_search_tool_bm25` as
advanced/deferred tool discovery helpers. DeepSeekCode's tool surface has grown
substantially, but all agent-visible tools are currently exposed up front and
there is no model-callable tool catalog search.

## 目标

- Add agent-visible `tool_search_tool_regex` and `tool_search_tool_bm25`.
- Search the static DeepSeek model tool schema catalog by name, description, and
  parameter schema.
- Return DeepSeek-TUI-style `tool_search_tool_search_result` payloads with
  `tool_reference` items.
- Support a bounded `limit` with a safe default.
- Expose both tools in the default registry and model schema.

## 非目标

- This slice does not defer/hydrate tools mid-conversation.
- This slice does not search dynamic MCP tool schemas.
- This slice does not add a full regex engine dependency; the regex tool uses a
  dependency-free common-pattern matcher suitable for tool discovery.

## 验收标准

1. `tool_search_tool_regex query=<pattern>` returns matching tool references.
2. `tool_search_tool_bm25 query=<terms>` ranks tools by local name/description
   relevance.
3. Empty queries are rejected.
4. Results omit the two search tools themselves.
5. The default registry exposes both tool names.
6. The model schema exposes both tool names.

## 实现结果

- Added `ToolSearchTool` in `src/tools/tool_search.rs` with two registered
  names: `tool_search_tool_regex` and `tool_search_tool_bm25`.
- Added `static_tool_search_catalog()` in `src/model/deepseek.rs` so the search
  tools use the same static tool schema catalog exposed to model providers.
- `tool_search_tool_regex` searches tool names, descriptions, and parameter
  schemas with dependency-free common pattern support for literal terms, `.*`,
  `.`, `|`, `^`, and `$`.
- `tool_search_tool_bm25` ranks matches with local term scoring over tool name,
  description, and parameter schema.
- Both tools accept `query` and optional bounded `limit`, omit the search tools
  themselves, and return `tool_search_tool_search_result` JSON with
  `tool_reference` items.
- Registered both tools in the default agent registry.
- Added both DeepSeek model schema entries.
- Updated runtime docs and the DeepSeek-TUI parity plan.

## 验证

- `cargo test tool_search`: passed, 4 tests.
- `cargo test default_registry_includes_read_only_git_history_tools`: passed.
- `cargo fmt --check`: passed.
- `git diff --check`: passed.
- `cargo test`: passed, 977 tests.
- `cargo package --allow-dirty`: passed, packaged 288 files and verified
  `deepseek_code v0.1.0`.
