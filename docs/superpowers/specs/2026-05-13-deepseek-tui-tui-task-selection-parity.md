# DeepSeek-TUI TUI Task Selection Parity Spec

Date: 2026-05-13

Source comparison: `Hmbown/DeepSeek-TUI`, `main` HEAD `3382242`

## Gap

The TUI task panel could show background task progress and command-palette
actions could pause, resume, or cancel tasks, but the foreground UI had no
selected-task state. Users still had to type exact ids or rely on status-first
fallbacks. A DeepSeek-TUI-style workbench should let the task panel itself act
as the control surface for background work.

## Scope

- Track a selected active-thread runtime task in the TUI state.
- Preserve the selected task across refreshes when it still exists, and reset it
  when switching to a thread without tasks.
- Render a visible selected-task marker in the task panel.
- Add command-palette navigation for `task next`, `task prev`, and
  `task select <id>`.
- Add mouse row selection for visible task-panel tasks.
- Make default `task pause`, `task resume`, and `task cancel` prefer the
  selected task when that task's status is compatible, then fall back to the
  existing status-first behavior.
- Document the selection controls and update the parity plan.

## Acceptance

1. A TUI loaded with active-thread tasks selects a deterministic default task.
2. The task panel marks the selected task with `>`.
3. `task select <id>` updates selected task state and rendering.
4. `task pause`, `task resume`, and `task cancel` without ids target the
   selected compatible task before falling back to status ordering.
5. Clicking a visible task row selects that task.
6. Existing runtime task panel and task control tests remain green.

## Implementation Notes

- Added `selected_task_id` to `TuiApp`.
- Added selection lifecycle helpers around active-thread task refresh.
- Updated task progress rendering to include a selected marker.
- Added command-palette task navigation and id selection.
- Added task panel mouse hit-testing for visible task rows.
- Reused the existing runtime task action path for selected-task defaults.

## Verification

- `/home/willamhou/.cargo/bin/cargo test selected_task --lib`
- `/home/willamhou/.cargo/bin/cargo test mouse_click_selects_task_panel_row --lib`
- `/home/willamhou/.cargo/bin/cargo test task --lib`
- `/home/willamhou/.cargo/bin/cargo test tui --lib`
- `/home/willamhou/.cargo/bin/cargo fmt --check`
- `git diff --check`
- `/home/willamhou/.cargo/bin/cargo test`
- `/home/willamhou/.cargo/bin/cargo package --allow-dirty`
