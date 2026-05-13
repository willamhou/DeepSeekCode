# DeepSeek-TUI MCP Runtime Automation Side Effects Parity

Date: 2026-05-13

Status: completed

## Gap

MCP server mode now exposes durable task queue writes, but runtime automation
metadata writes remain agent/HTTP/TUI-only. DeepSeek-TUI-style automation flows
need MCP clients to create, update, pause, resume, delete, and manually trigger
automations without bypassing the durable approval and audit model.

## Spec

1. Add durable-approval MCP tools:
   `runtime_create_automation`, `runtime_update_automation`,
   `runtime_pause_automation`, `runtime_resume_automation`,
   `runtime_delete_automation`, and `runtime_trigger_automation`.
2. Keep read-only automation listing/reading on existing HTTP/runtime paths; this
   slice only adds write/trigger MCP side effects.
3. Require durable MCP approvals for every new tool and append
   `permission_request` / `permission_response` before runtime mutation.
4. Reuse the existing runtime store validation for session/thread linkage,
   automation status, schedule aliases, trigger status, and task creation.
5. Document the MCP tool table and narrow Phase G2's remaining long-tail
   side-effect list.
6. Add focused tests for visibility and representative approved create,
   update/pause/resume/delete, and trigger flows.

## Implementation

- Added durable-approval MCP tools for runtime automation create, update,
  pause, resume, delete, and trigger.
- Routed every new automation MCP write through runtime permission requests
  before mutating `.dscode/runtime`.
- Reused existing runtime store validation and trigger-to-task creation.
- Updated runtime docs and the DeepSeek-TUI parity plan to mark runtime
  automation MCP side effects as landed.
- Strengthened the MCP test responder so multi-request approval tests answer the
  next unanswered permission request instead of re-answering old requests.

## Verification

- `/home/willamhou/.cargo/bin/cargo test mcp_tools_list_includes_write_tools_only_with_durable_approvals --lib`
- `/home/willamhou/.cargo/bin/cargo test mcp_tools_call_creates_and_triggers_runtime_automation_after_runtime_approval --lib`
- `/home/willamhou/.cargo/bin/cargo test mcp_tools_call_updates_pauses_resumes_and_deletes_runtime_automation_after_runtime_approval --lib`
- `/home/willamhou/.cargo/bin/cargo fmt --check`
- `git diff --check`
