# DeepSeek-TUI TUI Task Progress Detail Parity Spec

Date: 2026-05-13

Source comparison: `Hmbown/DeepSeek-TUI`, `main` HEAD `3382242`

## Gap

The TUI task panel showed only task kind, status, and summary. After adding
task-level pause, resume, and cancel controls, users still had to infer task ids
and recency from outside the panel. DeepSeek-TUI-style workbenches should make
background task control possible directly from the foreground task surface.

## Scope

- Render active-thread task status counts in the task panel.
- Render a short task id for each visible task.
- Render each task's `updated_at` timestamp next to its status.
- Keep existing task ordering and summary clipping.
- Document the richer progress display and update the parity plan.

## Acceptance

1. The task panel still shows the active-thread task count.
2. The panel includes status-count details such as `running=1`.
3. Each visible task line includes a task id usable with `task pause`,
   `task resume`, or `task cancel`.
4. Each visible task line includes the task's `updated_at` value.
5. Existing task panel tests remain green.

## Implementation Notes

- Added task panel formatting helpers in `src/tui.rs`.
- Updated runtime task panel rendering to include status counts and task ids.
- Extended the existing task panel unit test to assert id and update timestamp
  visibility.

## Verification

- `/home/willamhou/.cargo/bin/cargo test task_panel_renders_active_thread_runtime_tasks --lib`
- `/home/willamhou/.cargo/bin/cargo test app_from_store_loads_runtime_tasks_into_task_panel --lib`
- `/home/willamhou/.cargo/bin/cargo test tui --lib`
- `/home/willamhou/.cargo/bin/cargo fmt --check`
- `git diff --check`
- `/home/willamhou/.cargo/bin/cargo test`
- `/home/willamhou/.cargo/bin/cargo package --allow-dirty`
