# DeepSeek-TUI TUI Mouse Actions Parity Spec

Date: 2026-05-13

Source comparison: `Hmbown/DeepSeek-TUI`, `main` HEAD `3382242`

## Gap

DeepSeekCode's TUI had keyboard navigation for tabs, scrollback, session
selection, thread selection, and picker filtering, but the interactive terminal
did not capture mouse events. That left a visible workbench gap versus
DeepSeek-TUI-style terminal UIs where common navigation surfaces can be clicked
or scrolled directly.

## Scope

- Enable crossterm mouse capture while the interactive TUI is running and
  disable it during terminal teardown.
- Route mouse events through the same action drain path as keyboard events.
- Support mouse-wheel scrolling by reusing the active PageUp/PageDown target:
  command-independent picker navigation, MCP/detail scroll, or transcript
  scrollback.
- Support left-click mode switching on the Plan / Agent / YOLO tab bar.
- Support left-click session selection in the visible session picker.
- Support left-click thread selection in the visible thread navigator.
- Support left-click composer focus from the transcript panel.
- Keep this as a first-line mouse slice; richer drag, multi-select, and
  per-control mouse actions remain future work.
- Document the mouse controls and update the DeepSeek-TUI parity plan.

## Acceptance

1. Interactive TUI setup enables mouse capture and teardown disables it.
2. Clicking the tab bar changes `TuiMode` and updates status.
3. Clicking a visible session-picker row selects that session and closes the
   picker.
4. Clicking a visible thread-navigator row selects that thread and closes the
   navigator.
5. Mouse wheel events reuse the same navigation semantics as PageUp/PageDown.
6. Clicking the transcript body focuses composer input.
7. The broader TUI test group remains green.

## Implementation Notes

- Added `handle_mouse_event` and mouse-specific hit testing helpers.
- Stored the latest frame area during interactive draws so mouse coordinates
  can be mapped back to workbench regions.
- Reused existing layout helper functions for picker hit testing.
- Reused existing keyboard handlers for scroll-wheel behavior.
- Added focused tests for tab clicks, session picker row clicks, and thread
  picker row clicks plus wheel navigation.

## Verification

- `/home/willamhou/.cargo/bin/cargo test mouse_clicks_switch_mode_tabs`
- `/home/willamhou/.cargo/bin/cargo test mouse_click_transcript_focuses_composer`
- `/home/willamhou/.cargo/bin/cargo test session_picker_supports_mouse_selection`
- `/home/willamhou/.cargo/bin/cargo test thread_navigator_supports_mouse_selection_and_scroll`
- `/home/willamhou/.cargo/bin/cargo test tui`
- `/home/willamhou/.cargo/bin/cargo fmt --check`
- `git diff --check`
- `/home/willamhou/.cargo/bin/cargo test`
- `/home/willamhou/.cargo/bin/cargo package --allow-dirty`
