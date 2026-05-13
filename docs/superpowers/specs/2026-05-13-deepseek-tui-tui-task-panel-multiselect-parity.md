# DeepSeek-TUI TUI Task Panel Multi-Select Parity

Date: 2026-05-13

## Gap

DeepSeekCode's task panel supported a single selected runtime task for default
pause/resume/cancel actions. After MCP manager bulk selection landed, the
remaining TUI interaction gap was cross-surface multi-select outside the MCP
manager. Active-thread task rows still required repeated single-task commands
for bulk task control.

## Scope

- Add a task-panel multi-select set independent of the focused selected task.
- Preserve plain click semantics: left-click still selects one task for default
  single-task actions.
- Add Ctrl+click toggles for visible task rows.
- Add drag-select across visible task rows.
- Add command-palette selection helpers:
  - `task select all`
  - `task select clear`
  - `task bulk pause`
  - `task bulk resume`
  - `task bulk cancel`
- When selected tasks exist, default `task pause`, `task resume`, and
  `task cancel` apply to compatible selected tasks.
- Keep all existing single-task pause/resume/cancel behavior when no bulk
  selection exists.
- Document the behavior in `docs/tui.md` and the parity plan.

## Acceptance

- Ctrl+click toggles a task row in the selected set.
- Dragging across visible task rows selects the inclusive visible range.
- `task select all` selects visible task rows; `task select clear` clears the
  set.
- Bulk cancel queues one `CancelTask` action per selected compatible task in
  active task order.
- Existing selected-task default action behavior remains green.
- Focused and full Rust test gates pass.

## Verification

- `cargo test command_palette_bulk_selected_tasks_drive_default_actions --lib`
  passed.
- `cargo test mouse_ctrl_click_and_drag_select_task_panel_rows --lib` passed.
- `cargo test tui --lib` passed: 107 tests.
- `cargo fmt --check` passed.
- `git diff --check` passed.
- `cargo test` passed: 1074 tests.
- `cargo package --allow-dirty` passed.
