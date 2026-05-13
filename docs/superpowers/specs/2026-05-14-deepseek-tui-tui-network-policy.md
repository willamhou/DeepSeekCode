# DeepSeek-TUI TUI Network Policy

Status: implemented

## Gap

DeepSeek-TUI exposes `/network` commands for inspecting and editing persistent
host allow/deny policy from the terminal. DeepSeekCode already had
`network.default`, `network.allow`, and `network.deny` enforcement plus CLI
`config network allow|deny`, but the TUI had no local workbench surface for
listing, removing, or changing the default policy.

## Implementation

- Extended the config command helpers with reusable project-root operations for
  network policy summary, allow/deny host moves, host removal, and default
  policy updates.
- Added `TuiNetworkCommand` and a `Network` TUI action.
- Routed `network ...` and `/network ...` from both the command palette and the
  composer before custom slash-command fallback.
- Wired local file-backed TUI handling to update the selected session
  workspace `.dscode/config.toml`, then render the effective policy summary in
  the detail panel.
- Kept HTTP-runtime TUI behavior explicit: network policy edits require local
  file-backed access.
- Updated TUI documentation and the DeepSeek-TUI parity plan.

## Verification

- `cargo test network --lib`
- `cargo test composer_intercepts_memory_prefix_and_slash_commands --lib`
- `cargo test tui --lib`
- `cargo fmt --check`
- `cargo check`
- `git diff --check`

## Remaining

The TUI surface manages the same project policy as the CLI. User-level/global
network policy is not a separate concept in DeepSeekCode today.
