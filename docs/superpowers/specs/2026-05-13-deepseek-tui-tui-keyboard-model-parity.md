# DeepSeek-TUI TUI Keyboard Model Parity Spec

Date: 2026-05-13

Source comparison: `Hmbown/DeepSeek-TUI`, `main` HEAD `3382242`

## Gap

DeepSeekCode already has transcript scrollback and focused input editing in the
ratatui workbench, but the interactive loop passed only `KeyCode` into the app
state. That dropped terminal modifiers, so Ctrl-based editing could not work and
Ctrl-modified letters could accidentally trigger plain global one-key commands.

DeepSeek-TUI-style terminal workbenches need modifier-aware keyboard handling
for fast prompt editing and reliable modal input.

## Scope

- Preserve `crossterm::event::KeyEvent` modifiers in the TUI event loop.
- Add shared Ctrl editing controls for the composer and command palette:
  Ctrl+A, Ctrl+E, Ctrl+U, Ctrl+K, Ctrl+W, Ctrl+Left, and Ctrl+Right.
- Treat Ctrl+C as a TUI quit shortcut.
- Do not let unrelated Ctrl-modified letters fall through to global plain-key
  mode/session/thread commands.
- Document the input controls and update the DeepSeek-TUI parity plan.

## Acceptance

1. The interactive TUI dispatch path calls a modifier-aware handler.
2. Focused composer input supports Ctrl line/word editing.
3. Focused command-palette input supports the same Ctrl line/word editing.
4. Ctrl+A with no focused input does not switch to Agent mode.
5. The user-facing TUI docs list the modifier bindings.

## Implementation Notes

- Added `TuiApp::handle_key_event(KeyEvent)` and kept `handle_key(KeyCode)` for
  existing deterministic tests and plain key behavior.
- Added shared text helpers for start/end, clear-line, delete-to-end,
  delete-previous-word, and word-wise cursor motion.
- Updated `run_loop` to call `handle_key_event`.
- Updated `docs/tui.md` and the DeepSeek-TUI parity plan.

## Verification

- `/home/willamhou/.cargo/bin/cargo test composer_supports_control_key_editing`
- `/home/willamhou/.cargo/bin/cargo test command_palette_control_keys_edit_without_triggering_global_modes`
- `/home/willamhou/.cargo/bin/cargo test tui`
- `/home/willamhou/.cargo/bin/cargo fmt --check`
- `git diff --check`
- `/home/willamhou/.cargo/bin/cargo test`
- `/home/willamhou/.cargo/bin/cargo test -- --test-threads=1`
- `/home/willamhou/.cargo/bin/cargo package --allow-dirty`
