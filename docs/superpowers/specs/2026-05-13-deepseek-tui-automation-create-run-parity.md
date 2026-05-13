# DeepSeek-TUI Automation Create/Run Parity

Date: 2026-05-13

## Gap

DeepSeek-TUI exposes model-visible automation lifecycle tools, including
`automation_create` and `automation_run`, so the agent can create scheduled
durable automations and run one immediately after approval. DeepSeekCode already
had a durable runtime automation store plus `automation_list` and
`automation_read`, but the agent-visible tool surface could not create or run
automations directly.

## Scope

- Add agent-visible `automation_create`.
- Add agent-visible `automation_run`.
- Reuse the existing `.dscode/runtime` automation and task store.
- Accept DeepSeek-TUI-style `name`, `prompt`, and `rrule` creation inputs.
- Keep local compatibility with the existing `schedule`, `status`,
  `session_id`, `thread_id`, `last_run_at`, and `next_run_at` runtime fields.
- Support DeepSeek-TUI-style `paused` creation by mapping it to local
  `status=paused`.
- Make both tools write-approved runtime mutations, while keeping
  `automation_list` and `automation_read` approval-free.
- Expose OpenAI-compatible and Anthropic-compatible schemas.

## Acceptance

- `automation_create` and `automation_run` are present in the default registry.
- The model schema includes both tools, including `rrule` for creation and
  `automation_id` for running.
- `automation_create` creates an active automation from
  `name`/`prompt`/`rrule`.
- `automation_create` accepts the local `schedule` alias and `paused=true`.
- `automation_run` triggers an active automation and returns the updated
  automation plus the queued durable automation task.
- Registry permission previews report write requests for both creation and
  immediate runs.
- Runtime docs and the DeepSeek-TUI parity plan mention the new automation
  create/run tools.

## Implementation

- Added `AutomationCreateTool` and `AutomationRunTool` in
  `src/tools/runtime_tasks.rs`.
- Registered the tools in `src/tools/registry.rs`.
- Extended write-confirmation and permission-preview handling for automation
  mutations.
- Added static model schemas in `src/model/deepseek.rs`.
- Updated `docs/runtime.md` and the DeepSeek-TUI parity plan.

## Verification

- `cargo test automation_create --lib` passed: 3 tests.
- `cargo test build_tool_specs_include_runtime_task_and_automation_tools --lib`
  passed.
- `cargo test permission_request_for_reports_write_shell_and_mcp_prompts --lib`
  passed.
- `cargo test default_registry_includes_todo_checklist_compat_tools --lib`
  passed.
- `cargo fmt --check` passed.
- `git diff --check` passed.
- `cargo test` passed: 1082 library tests plus bin/doc-test targets.
- `cargo package --allow-dirty` passed.
