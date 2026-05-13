# DeepSeek-TUI Automation Lifecycle Parity

Date: 2026-05-13

## Gap

DeepSeek-TUI exposes the full model-visible automation lifecycle:
`automation_create`, `automation_list`, `automation_read`, `automation_update`,
`automation_pause`, `automation_resume`, `automation_delete`, and
`automation_run`. DeepSeekCode had durable automation storage and, after the
create/run slice, could create, list, read, and run automations, but could not
update, pause, resume, or delete them through the agent-visible tool surface.

## Scope

- Add runtime store operations for automation update, pause, resume, and delete.
- Add agent-visible `automation_update`, `automation_pause`,
  `automation_resume`, and `automation_delete`.
- Reuse the existing `.dscode/runtime/automations` JSON files.
- Record linked thread events for lifecycle mutations when an automation is
  attached to a runtime thread.
- Keep write approval for all lifecycle mutations.
- Expose DeepSeek-TUI-compatible schemas, including `automation_id`,
  `name`, `prompt`, `rrule`, `cwds`, and `status` where relevant.
- Preserve local compatibility with `schedule`, `paused`, and `next_run_at`.

## Acceptance

- The default registry includes all eight DeepSeek-TUI automation lifecycle
  tool names.
- Model schemas include `automation_update`, `automation_pause`,
  `automation_resume`, and `automation_delete`.
- `automation_update` mutates name, prompt, rrule/schedule, status/paused, and
  next-run metadata.
- `automation_pause` changes status to `paused`.
- `automation_resume` changes status to `active`.
- `automation_delete` removes the automation JSON record and reports a
  cancelled/deleted result.
- Permission previews report write requests for lifecycle mutations.
- Runtime docs and the DeepSeek-TUI parity plan list the full lifecycle surface.

## Implementation

- Added `RuntimeStore::{update_automation,pause_automation,resume_automation,delete_automation}`.
- Added thread-event recording for lifecycle mutation events.
- Added lifecycle tool wrappers in `src/tools/runtime_tasks.rs`.
- Registered the tools and write-approval handling in `src/tools/registry.rs`.
- Added static tool schemas in `src/model/deepseek.rs`.
- Updated docs and the parity plan.

## Verification

- `cargo test automation_update --lib` passed.
- `cargo test update_pause_resume_and_delete_automation_records_events --lib`
  passed.
- `cargo test build_tool_specs_include_runtime_task_and_automation_tools --lib`
  passed.
- `cargo test permission_request_for_reports_write_shell_and_mcp_prompts --lib`
  passed.
- `cargo test default_registry_includes_todo_checklist_compat_tools --lib`
  passed.
- `cargo fmt --check` passed.
- `git diff --check` passed.
- `cargo test` passed: 1084 library tests plus bin/doc-test targets.
- `cargo package --allow-dirty` passed.
