# DeepSeek-TUI TUI MCP Manager Mouse Parity Spec

Date: 2026-05-13

Source comparison: `Hmbown/DeepSeek-TUI`, `main` HEAD `3382242`

## Gap

The first TUI mouse slice added mode, picker, scroll, and composer-focus mouse
controls, but the full-width MCP manager still relied on keyboard shortcuts and
command strings. That left one of the most workbench-like DeepSeekCode screens
without direct mouse interaction.

## Scope

- Support left-click MCP manager tab switching.
- Support left-click server-row selection inside the full-width MCP manager.
- Support left-click action-strip commands for the selected server:
  - enable
  - disable
  - remove
  - tools
  - reload
- Reuse existing keyboard/config mutation paths and durable action queue
  semantics.
- Preserve existing `Tab` / `Shift+Tab`, `n` / `p`, `e` / `d` / `x` / `t` /
  `r`, filtering, and scroll behavior.
- Document the mouse controls and update the DeepSeek-TUI parity plan.

## Acceptance

1. Clicking a visible MCP manager tab queues the same manager-tab action as the
   keyboard tab commands.
2. Clicking a visible server row updates the selected server and status.
3. Clicking action-strip enable/disable queues `McpSetEnabled` for the selected
   server and scope.
4. Clicking action-strip tools queues `McpManagerDetails` for the selected
   server.
5. Clicking action-strip reload queues `McpList`.
6. Clicking action-strip remove opens the existing remove confirmation modal.
7. The broader TUI test group remains green.

## Implementation Notes

- Extended `handle_mouse_event` with MCP manager body hit testing.
- Reused rendered tab and action-strip text to map mouse columns to commands.
- Mapped visible server rows back to parsed MCP server entries.
- Added focused tests for tab/server-row clicks and action-strip clicks.

## Verification

- `/home/willamhou/.cargo/bin/cargo test mcp_manager_mouse_clicks_tabs_and_server_rows`
- `/home/willamhou/.cargo/bin/cargo test mcp_manager_mouse_action_strip_targets_selected_server`
- `/home/willamhou/.cargo/bin/cargo test tui`
- `/home/willamhou/.cargo/bin/cargo fmt --check`
- `git diff --check`
- `/home/willamhou/.cargo/bin/cargo test`
- `/home/willamhou/.cargo/bin/cargo package --allow-dirty`
