# DeepSeek-TUI MCP Web Run Parity

Date: 2026-05-13

Status: completed

## Gap

DeepSeekCode already had an agent-visible DeepSeek-TUI-compatible `web_run`
aggregate wrapper and runtime docs described it in the server tool table, but
`serve --mcp` and ACP `session/tools/list` only exposed the narrower
`web_search`, `fetch_url`, and `finance` tools.

## Spec

1. Expose `web_run` through MCP `tools/list` with schema fields for
   `search_query`, `open`, `click`, `find`, `finance`, `image_query`,
   `screenshot`, and `response_length`.
2. Route MCP `tools/call name=web_run` through the existing `WebRunTool`
   implementation so ACP inherits the same session-scoped tool bridge.
3. Preserve existing network policy behavior and avoid adding new trusted
   side-effect gates; `web_run` remains a read-only network tool.
4. Mark ACP `web_run` updates as a search-style tool kind.
5. Add focused tests for MCP list/call and ACP list visibility.

## Verification

- `/home/willamhou/.cargo/bin/cargo test mcp_tools_list_includes_workspace_and_runtime_tools --lib`
- `/home/willamhou/.cargo/bin/cargo test mcp_tools_call_executes_web_run_without_network_for_unsupported_actions --lib`
- `/home/willamhou/.cargo/bin/cargo test acp_session_tools_list_new_session_is_read_only --lib`
- `/home/willamhou/.cargo/bin/cargo fmt --check`
- `git diff --check`

## Implementation

- `execute_mcp_tool` now dispatches `web_run` to `WebRunTool`.
- `mcp_tool_definitions` advertises the aggregate `web_run` schema.
- ACP `session/tools/list` inherits `web_run` through the existing MCP state
  adapter, and ACP tool update kind mapping treats it as `search`.
