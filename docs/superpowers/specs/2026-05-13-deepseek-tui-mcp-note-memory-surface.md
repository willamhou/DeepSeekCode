# DeepSeek-TUI MCP Note/Memory Surface

Date: 2026-05-13

Status: completed

## Gap

`note` and `remember` were agent-visible DeepSeek-TUI compatibility helpers,
but MCP/ACP clients could not call them. Both append durable files, so exposing
them in default read-only MCP sessions would violate the MCP safety model.

## Spec

1. Keep `note` and `remember` hidden from default MCP/ACP sessions.
2. Expose `note` only when durable write approvals are available.
3. Expose `remember` only when `memory.enabled` is true and durable write
   approvals are available.
4. Route both helpers through `permission_request kind=write` before appending
   to the configured notes or memory file.
5. Reuse the existing `NoteTool` and `RememberTool` implementations, including
   empty-content validation and `remember` cleanup of leading `#`.
6. Add focused tests for default rejection, durable visibility, successful
   approved writes, and permission event recording.

## Verification

- `/home/willamhou/.cargo/bin/cargo test mcp_tools_call_rejects_memory_writes_until_durable_approvals --lib`
- `/home/willamhou/.cargo/bin/cargo test mcp_tools_call_executes_memory_writes_after_runtime_approval --lib`
- `/home/willamhou/.cargo/bin/cargo test mcp_tools_list_includes_workspace_and_runtime_tools --lib`
- `/home/willamhou/.cargo/bin/cargo test mcp_tools_list_includes_write_tools_only_with_durable_approvals --lib`
- `/home/willamhou/.cargo/bin/cargo test acp_loaded_session_tools_call_write_file_uses_runtime_approval --lib`
- `/home/willamhou/.cargo/bin/cargo test mcp_ --lib`
- `/home/willamhou/.cargo/bin/cargo test acp_ --lib`
- `/home/willamhou/.cargo/bin/cargo fmt --check`
- `git diff --check`

## Implementation

- MCP `tools/list` advertises `note` only in durable write-approval mode.
- MCP `tools/list` advertises `remember` only when memory is enabled and
  durable write approvals are available.
- MCP `tools/call` rejects default `note` / `remember` calls, records durable
  write approval requests, and then delegates to the existing local tools after
  approval.
- ACP inherits the same gated visibility and write-approval behavior through
  its session-scoped MCP adapter.
