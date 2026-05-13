# DeepSeek-TUI TUI MCP Manager Bulk Actions Parity Spec

Date: 2026-05-13

Source comparison: `Hmbown/DeepSeek-TUI`, `main` HEAD `3382242`

## Gap

The full-width MCP manager had keyboard and mouse affordances for one selected
server at a time. That still left larger MCP configurations slower to manage
than DeepSeek-TUI-style workbench surfaces, where repeated server operations
should not require moving through every row one by one.

## Scope

- Add MCP manager server multi-select state.
- Add keyboard controls:
  - `Space` toggles the current selected server in the multi-select set.
  - `A` selects all currently visible server rows.
  - `U` clears the multi-select set.
  - `E` bulk-enables selected servers.
  - `D` bulk-disables selected servers.
- Add Ctrl+left-click server-row toggling for mouse multi-select.
- Show selected-server count in the MCP manager action strip.
- Make enable/disable action-strip clicks apply to the multi-select set when
  it is non-empty; keep existing single-selected-server behavior otherwise.
- Reuse existing per-server `McpSetEnabled` actions and project/user scope
  checks.
- Document the controls and update the DeepSeek-TUI parity plan.

## Acceptance

1. `Space` toggles the current selected server into or out of the multi-select
   set.
2. `A` selects all visible server rows and `U` clears the set.
3. `E` / `D` queue one `McpSetEnabled` action per selected project/user server.
4. Ctrl+left-click on a server row toggles that server in the multi-select set.
5. Enable/disable action-strip clicks apply to selected servers when the set is
   non-empty.
6. Existing single-server keyboard and mouse actions continue to work when the
   multi-select set is empty.
7. The broader TUI test group remains green.

## Implementation Notes

- Added `mcp_manager_selected_server_keys` to `TuiApp`.
- Added selection helpers for current, visible, and selected MCP server rows.
- Added bulk enable/disable request routing that reuses `TuiAction::McpSetEnabled`.
- Extended MCP manager mouse handling to use Ctrl+click as a multi-select
  toggle.
- Updated MCP manager action-strip rendering to expose selected count and bulk
  key hints.
- Added focused tests for keyboard bulk enable/disable and Ctrl+click plus
  action-strip bulk enable.

## Verification

- `/home/willamhou/.cargo/bin/cargo test mcp_manager_keyboard_bulk_selects_and_sets_enabled`
- `/home/willamhou/.cargo/bin/cargo test mcp_manager_mouse_ctrl_click_toggles_bulk_selection`
- `/home/willamhou/.cargo/bin/cargo test tui`
- `/home/willamhou/.cargo/bin/cargo fmt --check`
- `git diff --check`
- `/home/willamhou/.cargo/bin/cargo test`
- `/home/willamhou/.cargo/bin/cargo package --allow-dirty`
