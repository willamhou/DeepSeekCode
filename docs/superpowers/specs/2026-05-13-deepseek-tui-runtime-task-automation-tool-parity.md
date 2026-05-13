# DeepSeek-TUI Runtime Task/Automation Tool Parity Spec

日期：2026-05-13

对比对象：`Hmbown/DeepSeek-TUI`，`main` HEAD `3382242`

## 背景

DeepSeek-TUI exposes durable work tools such as `task_create`, `task_list`,
`task_read`, `task_cancel`, plus read-only `automation_list` and
`automation_read`. DeepSeekCode already has a durable runtime store and MCP
`runtime_list_tasks` / `runtime_read_task`, but agent runs do not expose the
DeepSeek-TUI-compatible tool names.

## 目标

- Add agent-visible `task_create`, `task_list`, `task_read`, and `task_cancel`.
- Add agent-visible `automation_list` and `automation_read`.
- Back the tools with the existing `.dscode/runtime` `RuntimeStore`.
- Support common DeepSeek-TUI parameter names: `prompt`, `task_id`,
  `automation_id`, and `limit`.
- Support DeepSeekCode scoping parameters where available: `session_id`,
  `thread_id`, `parent_task_id`, `kind`, and `status`.
- Require write approval for `task_create` and `task_cancel`.
- Keep `task_list`, `task_read`, `automation_list`, and `automation_read`
  read-only and approval-free.

## 非目标

- This slice does not implement `task_gate_run`.
- This slice does not implement `task_shell_start` / `task_shell_wait`.
- This slice does not implement PR-attempt tools.
- This slice does not create or mutate automation schedules.

## 验收标准

1. `task_create prompt=<text>` creates a pending runtime task and returns JSON.
2. `task_list` lists recent runtime tasks with optional session/thread filters.
3. `task_read task_id=<id>` returns one runtime task.
4. `task_cancel task_id=<id>` cancels a pending/running task and records runtime
   cancellation data when linked to a thread.
5. `automation_list` and `automation_read automation_id=<id>` return durable
   automation records.
6. Registry/model schemas expose all six names.
7. `task_create` and `task_cancel` produce write permission requests under the
   default write-confirmation policy.

## 实现结果

- Added `src/tools/runtime_tasks.rs` with `task_create`, `task_list`,
  `task_read`, `task_cancel`, `automation_list`, and `automation_read`.
- All six tools use the existing `.dscode/runtime` `RuntimeStore`.
- `task_create` accepts required `prompt` plus a `summary` alias, optional
  `session_id`, `thread_id`, `parent_task_id`, `kind`, and `status`, and
  defaults to `kind=agent`, `status=pending`.
- `task_list` accepts optional `session_id`, `thread_id`, and bounded `limit`.
- `task_read` accepts `task_id` or `id`.
- `task_cancel` accepts `task_id` or `id`, plus optional `reason`, and returns
  the cancelled task plus any runtime cancel event.
- `automation_list` accepts optional `session_id`, `thread_id`, and bounded
  `limit`.
- `automation_read` accepts `automation_id` or `id`.
- Registered all six tools in the default runtime registry.
- Added write permission requests and direct confirmation prompts for
  `task_create` and `task_cancel`; read/list tools remain approval-free.
- Added static model schemas for all six tools.
- Documented the tools in `docs/runtime.md` and the parity plan.

## 验证

- `/home/willamhou/.cargo/bin/cargo test runtime_tasks` passed: 4 matching tests.
- `/home/willamhou/.cargo/bin/cargo test build_tool_specs_include_runtime_task_and_automation_tools` passed.
- `/home/willamhou/.cargo/bin/cargo test default_registry_includes_read_only_git_history_tools` passed.
- `/home/willamhou/.cargo/bin/cargo test permission_request_for_reports_write_shell_and_mcp_prompts` passed.
- `/home/willamhou/.cargo/bin/cargo fmt --check` passed.
- `git diff --check` passed.
- `/home/willamhou/.cargo/bin/cargo test` passed: 964 tests.
- `/home/willamhou/.cargo/bin/cargo package --allow-dirty` passed: packaged
  282 files and verified `deepseek_code v0.1.0`.
