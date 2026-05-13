# DeepSeek-TUI TUI Picker Filter Parity Spec

Date: 2026-05-13

Source comparison: `Hmbown/DeepSeek-TUI`, `main` HEAD `3382242`

## Gap

DeepSeekCode's session picker and thread navigator could move quickly after the
picker-navigation slice, but they still showed the full durable runtime list.
That leaves large workspaces slower to scan than DeepSeek-TUI-style terminal
workbench flows, where selection surfaces need narrowing as well as movement.

## Scope

- Add persistent session-picker filter state.
- Add persistent thread-navigator filter state.
- Add command-palette commands to set or clear each filter:
  - `session filter <query>` / `session filter`
  - `thread filter <query>` / `thread filter`
- Render filter labels and empty-match states in both picker surfaces.
- Keep picker keyboard navigation bounded to the filtered result set.
- Preserve existing session/thread opening, Enter selection, cross-picker
  switching, and PageUp/PageDown/Home/End behavior.
- Document the commands and update the DeepSeek-TUI parity plan.

## Acceptance

1. `session filter <query>` opens the session picker, stores the query, selects
   the first matching session when needed, and renders only matching sessions.
2. `session filter` clears the session filter and restores the full list.
3. `thread filter <query>` opens the thread navigator, stores the query,
   selects the first matching thread when needed, and renders only matching
   current-session threads.
4. `thread filter` clears the thread filter and restores the full current
   session thread list.
5. Picker Up/Down/PageUp/PageDown/Home/End navigation remains clamped to
   visible matches.
6. The broader TUI test group remains green.

## Implementation Notes

- Added `session_picker_filter` and `thread_picker_filter` to `TuiApp`.
- Added filter helpers for session and thread metadata.
- Routed picker navigation through filtered index lists.
- Added command completions and command-palette execution branches.
- Updated picker rendering to show filter labels and empty-match placeholders.
- Added focused tests for session and thread filter behavior.

## Verification

- `/home/willamhou/.cargo/bin/cargo test session_picker_filters_sessions_from_command_palette`
- `/home/willamhou/.cargo/bin/cargo test thread_navigator_filters_threads_from_command_palette`
- `/home/willamhou/.cargo/bin/cargo test tui`
- `/home/willamhou/.cargo/bin/cargo fmt --check`
- `git diff --check`
- `/home/willamhou/.cargo/bin/cargo test`
- `/home/willamhou/.cargo/bin/cargo package --allow-dirty`
