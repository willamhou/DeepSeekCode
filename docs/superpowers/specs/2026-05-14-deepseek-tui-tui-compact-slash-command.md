# DeepSeek-TUI TUI Compact Slash Command

Status: implemented

## Gap

DeepSeek-TUI registers `/compact` as a first-class slash command. DeepSeekCode
could compact the active durable thread from the command palette with
`compact [tail]`, but composer input `/compact` fell through to custom slash
command handling instead of queuing a local compaction action.

## Implementation

- Added built-in parsing for `compact [tail]` and `/compact [tail]`.
- Kept `thread compact [tail]` as a DeepSeekCode-friendly alias.
- Routed all forms to the existing `TuiAction::CompactThread` path.
- Updated TUI help usage, docs, and the DeepSeek-TUI parity plan.

## Verification

- `cargo test command_palette_requests_thread_compaction --lib`
- `cargo test tui --lib`
- `cargo fmt --check`
- `cargo check`
- `git diff --check`

## Remaining

No known `/compact` command-name parity blocker remains.
