# DeepSeek-TUI TUI Verbose Command

Status: implemented

## Gap

DeepSeek-TUI exposes `/verbose [on|off]` to toggle whether live thinking renders
in full. DeepSeekCode persisted reasoning items and exposed a reasoning browser,
but the main transcript always rendered the first reasoning line directly and
had no operator control for compact versus verbose live thinking display.

## Implementation

- Added built-in `verbose` / `/verbose` parsing before custom slash-command
  fallback.
- Supported `on`, `off`, `toggle`, and `show` arguments, with no argument
  matching DeepSeek-TUI's toggle behavior.
- Added local `verbose_transcript` state to `TuiApp`, defaulting to off.
- Rendered reasoning transcript entries compactly by default, while
  `/verbose on` restores fuller reasoning text in the live transcript.
- Added a right-side `Verbose Transcript` detail panel with current state,
  active reasoning item count, and command usage.
- Routed verbose commands from both command palette and focused composer without
  starting a model turn.
- Added command-palette and composer slash completions, help text, settings
  detail, TUI docs, and the DeepSeek-TUI parity plan entry.

## Verification

- `cargo test verbose_command_toggles_reasoning_transcript_detail --lib`
- `cargo test tui --lib`
- `cargo fmt --check`
- `cargo check`
- `git diff --check`

## Remaining

This is session-local like the current local theme state. Persisted verbose
transcript preference can be added later if we introduce a broader TUI settings
write path.
