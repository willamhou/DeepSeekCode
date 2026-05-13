# DeepSeek-TUI TUI Picker Navigation Parity Spec

Date: 2026-05-13

Source comparison: `Hmbown/DeepSeek-TUI`, `main` HEAD `3382242`

## Gap

DeepSeekCode's session picker and thread navigator supported only single-step
Up/Down navigation. That is enough for tiny demos, but it is slower than a
terminal workbench should be once durable runtime sessions and threads grow.

## Scope

- Add PageUp/PageDown navigation to the session picker.
- Add Home/End navigation to the session picker.
- Add PageUp/PageDown navigation to the thread navigator.
- Add Home/End navigation to the thread navigator.
- Keep existing `j`/`k`, Up/Down, Enter, and cross-picker `s`/`t` behavior.
- Document the key behavior and update the DeepSeek-TUI parity plan.

## Acceptance

1. Session picker PageDown advances by a bounded page, clamped to the last
   session.
2. Session picker PageUp moves back by a bounded page, clamped to the first
   session.
3. Session picker Home/End jump to first/last session.
4. Thread navigator PageDown/PageUp and Home/End provide equivalent bounded
   navigation within the selected session's threads.
5. The broader TUI test group remains green.

## Implementation Notes

- Added `TUI_PICKER_PAGE_SIZE`.
- Extended `handle_session_picker_key`.
- Extended `handle_thread_picker_key`.
- Added focused tests for both pickers.

## Verification

- `/home/willamhou/.cargo/bin/cargo test session_picker_supports_page_and_edge_navigation`
- `/home/willamhou/.cargo/bin/cargo test thread_navigator_supports_page_and_edge_navigation`
- `/home/willamhou/.cargo/bin/cargo test tui`
- `/home/willamhou/.cargo/bin/cargo fmt --check`
- `git diff --check`
- `/home/willamhou/.cargo/bin/cargo test`
- `/home/willamhou/.cargo/bin/cargo package --allow-dirty`
