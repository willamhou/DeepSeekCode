# DeepSeek-TUI Agent Lifecycle Tool Parity

Date: 2026-05-13

## Gap

DeepSeek-TUI exposes model-visible background sub-agent lifecycle tools:
`agent_spawn`, `agent_result`, `agent_list`, `agent_cancel`, `close_agent`,
`resume_agent`, and `send_input`. DeepSeekCode had synchronous
`dispatch_subagent` and `dispatch_subagents`, but did not expose the same
lifecycle tool names or durable status/result handles.

## Scope

- Add agent-visible `agent_spawn`.
- Add read tools `agent_result` and `agent_list`.
- Add mutation tools `agent_cancel`, `close_agent`, `resume_agent`, and
  `send_input`.
- Back the first implementation with existing `.dscode/runtime` durable
  threads and tasks.
- Have `agent_spawn` create a runtime thread plus pending `subagent` task and
  return `agent_id` immediately.
- Have `send_input` append a user message to the agent thread and enqueue a
  follow-up `subagent_input` task.
- Keep lifecycle mutations write-approved.
- Expose OpenAI-compatible and Anthropic-compatible schemas.

## Acceptance

- All seven lifecycle tool names are present in the default registry.
- Model schemas include all seven lifecycle tool names.
- `agent_spawn` returns an `agent_id` and a pending durable `subagent` task.
- `agent_result` reads a sub-agent snapshot by `agent_id`.
- `agent_list` lists runtime-backed sub-agent tasks.
- `agent_cancel` cancels a pending/running sub-agent task.
- `close_agent` is accepted as a DeepSeek-TUI-compatible close/cancel alias.
- `resume_agent` requeues paused or completed/cancelled sub-agent work.
- `send_input` queues a follow-up `subagent_input` task on the same thread.
- Runtime docs and the DeepSeek-TUI parity plan mention the lifecycle surface.

## Implementation

- Added runtime-backed lifecycle tool structs in `src/tools/runtime_tasks.rs`.
- Registered lifecycle tools in `src/tools/registry.rs`.
- Added write-approval permission previews for lifecycle mutations.
- Added static model schemas in `src/model/deepseek.rs`.
- Updated runtime docs and the DeepSeek-TUI parity plan.

## Verification

- `cargo test agent_lifecycle --lib` passed: 2 tests.
- `cargo test build_tool_specs_include_agent_lifecycle_tools --lib` passed.
- `cargo test permission_request_for_reports_write_shell_and_mcp_prompts --lib`
  passed.
- `cargo test default_registry_includes_todo_checklist_compat_tools --lib`
  passed.
- `cargo fmt --check` passed.
- `git diff --check` passed.
- `cargo test` passed: 1088 library tests plus bin/doc-test targets.
- `cargo package --allow-dirty` passed.
