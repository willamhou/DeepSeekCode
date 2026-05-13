# DeepSeek-TUI MCP Runtime Agent Lifecycle Parity

Date: 2026-05-13

Status: completed

## Gap

DeepSeekCode already has DeepSeek-TUI-compatible sub-agent lifecycle tools on
the agent-visible registry (`agent_spawn`, `agent_result`, `agent_list`,
`agent_cancel`, `close_agent`, `resume_agent`, and `send_input`). They map to
durable runtime threads/tasks, but `serve --mcp` still cannot drive the same
runtime sub-agent lifecycle.

## Spec

1. Add read-only MCP tools `runtime_list_agents` and `runtime_agent_result`.
2. Add durable-approval MCP tools `runtime_spawn_agent`,
   `runtime_cancel_agent`, `runtime_close_agent`, `runtime_resume_agent`, and
   `runtime_send_agent_input`.
3. Keep these tools at the runtime metadata layer: spawn/resume/send enqueue
   pending runtime tasks and append user input items, but do not directly run a
   child model process.
4. Route every mutating tool through `permission_request` /
   `permission_response` before writing runtime records.
5. Reuse the same sub-agent task semantics as the agent-visible lifecycle tools:
   sub-agent task kinds are `subagent` and `subagent_input`.
6. Document the MCP tool table and narrow Phase G2's remaining long-tail
   side-effect list.
7. Add focused tests for visibility and representative approved spawn,
   send-input, cancel/close, and resume flows.

## Implementation

- Added read-only MCP `runtime_list_agents` and `runtime_agent_result`.
- Added durable-approval MCP `runtime_spawn_agent`, `runtime_cancel_agent`,
  `runtime_close_agent`, `runtime_resume_agent`, and
  `runtime_send_agent_input`.
- Kept execution at the runtime metadata layer: spawn/resume/send enqueue
  runtime tasks and append user input, but do not directly run child model
  processes.
- Routed every mutating lifecycle call through runtime `permission_request` /
  `permission_response` before writing runtime records.
- Updated runtime docs and the DeepSeek-TUI parity plan to mark MCP sub-agent
  lifecycle parity as landed.
- Added focused tests for tools/list visibility, spawn/list/result/send-input,
  and cancel/close/resume flows.

## Verification

- `/home/willamhou/.cargo/bin/cargo test mcp_tools_list_includes_workspace_and_runtime_tools --lib`
- `/home/willamhou/.cargo/bin/cargo test mcp_tools_list_includes_write_tools_only_with_durable_approvals --lib`
- `/home/willamhou/.cargo/bin/cargo test mcp_tools_call_spawns_lists_reads_and_sends_runtime_agent_input_after_runtime_approval --lib`
- `/home/willamhou/.cargo/bin/cargo test mcp_tools_call_cancels_closes_and_resumes_runtime_agents_after_runtime_approval --lib`
- `/home/willamhou/.cargo/bin/cargo fmt --check`
- `git diff --check`
