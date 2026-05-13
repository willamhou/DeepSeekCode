# DeepSeek-TUI TUI Mode Command

Status: implemented

## Gap

DeepSeek-TUI exposes `/mode [agent|plan|yolo|1|2|3]` for command-driven mode
switching. DeepSeekCode had Plan / Agent / YOLO tabs, keyboard shortcuts, mouse
switching, and command-palette-only `mode plan|agent|yolo`, but the composer
slash path and numeric aliases were missing.

## Implementation

- Added built-in `mode` / `/mode` parsing before custom slash-command fallback.
- Added DeepSeek-TUI-compatible target parsing for `agent|plan|yolo|1|2|3`.
- Added a no-target mode detail panel with current mode, available commands,
  and keyboard shortcuts using `TuiMcpDetailKind::Mode`.
- Routed mode commands from both command palette and focused composer without
  starting a model turn.
- Added command-palette and composer slash completions for mode commands.
- Updated TUI documentation and the DeepSeek-TUI parity plan.

## Verification

- `cargo test mode_command_shows_and_switches_modes --lib`
- `cargo test tui --lib`
- `cargo fmt --check`
- `cargo check`
- `git diff --check`

## Remaining

DeepSeekCode shows a terminal-native detail panel for `/mode` instead of opening
DeepSeek-TUI's picker modal. Direct target selection and keyboard/mouse mode
controls are covered.
