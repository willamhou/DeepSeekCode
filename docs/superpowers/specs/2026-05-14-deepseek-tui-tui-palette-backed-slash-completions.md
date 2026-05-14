# DeepSeek-TUI TUI Palette-Backed Slash Completions

Status: implemented

## Gap

DeepSeek-TUI makes slash commands discoverable while typing. DeepSeekCode could
execute composer-entered `/compact`, `/mcp`, `/jobs`, and `/restore` forms, but
the composer slash hint and Tab-completion catalog did not advertise the
palette-backed `/mcp`, `/jobs`, `/restore` families, and `/compact` was also
missing from the slash completion list.

## Implementation

- Added `/compact` completion entries.
- Added DeepSeek-TUI-style `/mcp ...`, `/jobs ...`, and `/restore ...`
  completion entries matching the built-in command palette dispatcher surface.
- Kept execution routed through the existing parser/dispatcher paths; this
  slice only changes command discoverability.
- Updated TUI docs and the DeepSeek-TUI parity plan.

## Verification

- `cargo test composer_slash_hints_include_deepseek_tui_palette_backed_commands --lib`
- `cargo test tui --lib`
- `cargo fmt --check`
- `cargo check`
- `git diff --check`

## Remaining

No known DeepSeek-TUI slash completion gap remains for `/compact`, `/mcp`,
`/jobs`, or `/restore`.
