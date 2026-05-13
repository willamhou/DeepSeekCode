# DeepSeek-TUI TUI MCP Manager Drag-Select Parity

Date: 2026-05-13

## Gap

DeepSeekCode's MCP manager already supported keyboard multi-select and
Ctrl+click row toggles, but it did not support drag-selecting visible rows. The
DeepSeek-TUI parity plan still tracked richer drag-selection workflows as a TUI
affordance gap.

## Scope

- Add MCP manager drag state for visible server rows.
- Preserve existing normal click behavior: a plain click selects the current
  server for action-strip commands but does not add it to the bulk selection.
- On left-drag across server rows, add the visible row range from the drag
  anchor to the current row into the bulk selection set.
- Clear the drag anchor on left-button release and when replacing/closing MCP
  manager detail.
- Keep existing Ctrl+click, keyboard multi-select, and action-strip behavior.
- Document drag-select in `docs/tui.md` and the parity plan.

## Acceptance

- Dragging from one visible server row to another selects the inclusive visible
  row range for bulk actions.
- Plain row clicks still only move the selected server.
- Ctrl+click row toggles still work.
- MCP manager mouse/action tests remain green.
- Focused and full Rust test gates pass.

## Verification

- `cargo test mcp_manager_mouse_drag_selects_visible_server_range --lib`: passed.
- `cargo test mcp_manager_mouse --lib`: 4 passed.
- `cargo test tui --lib`: 105 passed.
- `cargo fmt --check`: passed.
- `git diff --check`: passed.
- `cargo test`: 1072 passed.
- `cargo package --allow-dirty`: passed.
