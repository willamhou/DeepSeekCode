# DeepSeek-TUI TUI Exit Command

Status: implemented

## Gap

DeepSeek-TUI exposes `/exit` with `quit` and `q` aliases. DeepSeekCode already
quit on keyboard `q` / `Ctrl+C`, but the command palette and focused composer
did not recognize the slash-style exit commands.

## Implementation

- Added built-in `exit` / `/exit`, `quit` / `/quit`, and `q` / `/q` parsing.
- Routed exit commands from both command palette and focused composer before
  custom slash-command fallback.
- Added help metadata and command-palette/composer completions.
- Updated TUI documentation and the DeepSeek-TUI parity plan.

## Verification

- `cargo test exit_command_quits_from_palette_and_composer --lib`
- `cargo test tui --lib`
- `cargo fmt --check`
- `cargo check`
- `git diff --check`

## Remaining

No known remaining gap for the exit command surface.
