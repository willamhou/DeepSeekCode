# Claude Code REPL History Line Editor

## Context

The original REPL design intentionally left readline-style history for a later
slice. That remained a visible gap versus Claude Code CLI and other coding
agent CLIs: repeated prompts and slash commands could not be recalled with
arrow keys unless users wrapped the binary with an external readline helper.

## Scope

- Add built-in history browsing for `deepseek chat`, `deepseek repl`, and
  `deepseek interactive` when running in a real TTY.
- Keep scripted fixtures on the existing buffered-reader path.
- Avoid adding a new dependency; reuse the existing `crossterm` raw-mode/event
  stack already used by the TUI.
- Preserve basic local editing while in raw mode.

## Implementation

- `Repl::run` now uses a small raw-mode line editor for real TTY input.
- Interactive REPL turns install a SIGINT-backed cancellation flag and pass it
  through `AgentLoopOptions.cancel_check`.
- The editor supports:
  - Up / Down history browsing;
  - draft restore after browsing back down past the newest history item;
  - Left / Right, Home / End, Backspace, Delete;
  - Ctrl+A, Ctrl+E, Ctrl+U, Ctrl+K, Ctrl+W;
  - Ctrl+D on an empty line and Ctrl+C at the prompt to exit.
- The interactive reader decodes common raw PTY ANSI cursor sequences for Up,
  Down, Left, Right, Home, End, and Delete, and accepts Enter as `KeyCode::Enter`,
  LF/CR characters, or the terminal-equivalent Ctrl+J/Ctrl+M events.
- Submitted nonblank lines are stored in in-memory history, with consecutive
  duplicates collapsed.
- Ctrl+C during a running turn cancels cooperatively through the same
  `cancel_check` used by model streams and cancel-aware tools. Cancelled REPL
  turns restore the pre-turn transcript/snapshot pointer so the next prompt does
  not inherit a half-recorded user turn.
- `run_with_reader` remains unchanged for non-interactive tests and fixtures.

## Verification

- `cargo test line_editor --lib -- --test-threads=1`
- `cargo test cancel --lib -- --test-threads=1`
- `cargo test repl --lib -- --test-threads=1`
- `printf '/help\n\033[A\n/quit\n' | timeout 20s script -qfec 'cargo run --quiet -- chat' /tmp/deepseek-repl-history.typescript`
- `cargo fmt --check`
- `git diff --check`
- `cargo check --all-targets`
- `cargo check --target x86_64-pc-windows-gnu --all-targets`

## Remaining

Ctrl+C cancellation is cooperative. Model streaming and `run_shell` already poll
the flag; any future long-running tool that blocks without polling still needs
tool-local cancellation support.
