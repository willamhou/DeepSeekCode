# DeepSeek-TUI ACP Live Shell Streaming

Date: 2026-05-13

Status: completed

## Gap

ACP `session/tools/call` emitted standard tool call notifications and bounded
post-execution progress chunks for large outputs, but shell stdout/stderr still
arrived after the command finished. That left ACP clients without true
process-level terminal output while a shell tool was still executing.

## Spec

1. Keep default `session/tools/call` behavior unchanged for compatibility.
2. For `exec_shell` and `task_shell_start`, clients can pass `stream=true` or
   `follow=true` to opt into live process output.
3. Streaming `exec_shell` calls force `background=true`, then poll the matching
   `exec_shell_wait` background job while the process is running.
4. Streaming `task_shell_start` calls start the existing background job and poll
   `task_shell_wait` while the process is running.
5. Each live stdout/stderr delta is flushed as a partial
   `sessionUpdate: "tool_call_update"` before the final JSON-RPC result.
6. Loaded ACP sessions keep durable runtime audit records: one assistant
   `tool_call` item, one final `tool_result` item, and permission requests linked
   to the same turn.
7. Streaming shell calls remain side-effect gated; plain read-only ACP sessions
   return an error result instead of starting a shell process.
8. The final completion update and JSON-RPC result still include the complete
   shell snapshot.

## Verification

- `/home/willamhou/.cargo/bin/cargo test acp_session_tools_call_streams_shell_output_while_running --lib`
- `/home/willamhou/.cargo/bin/cargo test acp_session_tools_call_streaming_shell_requires_loaded_runtime_thread --lib`
- `/home/willamhou/.cargo/bin/cargo test acp_session_tools_call_streams_large_tool_output_updates --lib`
- `/home/willamhou/.cargo/bin/cargo test acp_loaded_session_tools_call_write_file_uses_runtime_approval --lib`
- `/home/willamhou/.cargo/bin/cargo test acp_ --lib`
- `/home/willamhou/.cargo/bin/cargo test mcp_ --lib`
- `/home/willamhou/.cargo/bin/cargo fmt --check`
- `git diff --check`

## Implementation

- ACP stdio detects opt-in streaming `session/tools/call` requests before the
  normal dispatcher and handles only `exec_shell` / `task_shell_start`.
- The adapter writes the initial `tool_call` update, starts the shell job through
  the existing approved shell tool path, polls non-blocking wait helpers, flushes
  partial `tool_call_update` stdout/stderr deltas, then writes the final
  completion update and JSON-RPC result.
- The streaming path reuses existing safe-command, durable approval, and runtime
  item recording policies instead of adding a separate shell execution path.
