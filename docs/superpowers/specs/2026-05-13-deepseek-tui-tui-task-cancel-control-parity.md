# DeepSeek-TUI TUI Task Cancel Control Parity Spec

Date: 2026-05-13

Source comparison: `Hmbown/DeepSeek-TUI`, `main` HEAD `3382242`

## Gap

The TUI task panel could create, pause, and resume durable runtime tasks, while
`cancel` only targeted the active running assistant turn. That left queued,
paused, daemon-claimed, or externally created tasks without a direct workbench
cancel control even though the runtime already had a durable task cancellation
endpoint.

## Scope

- Add a `CancelTask` TUI action.
- Add command palette forms:
  - `task cancel`
  - `task cancel <id>`
  - `tasks cancel`
  - `tasks cancel <id>`
- When `id` is omitted, choose the first active-thread task in this order:
  running, pending, paused.
- Route local file-backed TUI cancellation through `RuntimeStore::cancel_task`.
- Route HTTP-runtime TUI cancellation through `POST /v1/tasks/{id}/cancel`.
- Keep `cancel` / `stop` focused on the active running assistant turn.
- Document the new task control and update the parity plan.

## Acceptance

1. `task cancel` queues a task-cancel action for the first running active-thread
   task.
2. `task cancel <id>` targets that active-thread task by id.
3. Completed or failed tasks are rejected in the TUI before action dispatch.
4. Local handling marks the task `cancelled` and appends a linked
   `cancel_requested` event with `task_id`.
5. HTTP handling posts to the first-class runtime task cancel endpoint.
6. Existing `cancel` behavior for active assistant turns is unchanged.

## Implementation Notes

- Added `TuiAction::CancelTask`.
- Added task-cancel command parsing, completion hints, default task selection,
  and status messages in `src/tui.rs`.
- Added local and HTTP action handlers in `src/cli/commands/tui.rs`.
- Added focused command-palette and runtime-action tests.

## Verification

- `/home/willamhou/.cargo/bin/cargo test command_palette_requests_running_task_cancel_by_default --lib`
- `/home/willamhou/.cargo/bin/cargo test handle_tui_action_cancels_runtime_task --lib`
- `/home/willamhou/.cargo/bin/cargo test handle_tui_http_action_cancels_remote_task --lib`
- `/home/willamhou/.cargo/bin/cargo test tui --lib`
- `/home/willamhou/.cargo/bin/cargo fmt --check`
- `git diff --check`
- `/home/willamhou/.cargo/bin/cargo test`
- `/home/willamhou/.cargo/bin/cargo package --allow-dirty`
