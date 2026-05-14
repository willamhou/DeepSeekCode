# DeepSeek-TUI TUI Slash Command Registry Audit

Status: implemented

## Gap

DeepSeekCode had source-level comparison notes for the DeepSeek-TUI slash
command registry, but the audit was mostly manual. That let command execution
and completion drift apart, as seen with palette-backed slash commands that
were executable but not discoverable in composer hints.

## Implementation

- Added a focused TUI unit test with the DeepSeek-TUI command registry names
  from the refreshed 2026-05-14 source comparison.
- The test asserts that every upstream first-class slash command name has a
  matching composer slash hint or subcommand prefix.
- Kept the audit scoped to command discoverability; execution behavior remains
  covered by each command family's focused tests.

## Verification

- `cargo test composer_slash_hints_cover_deepseek_tui_command_registry_names --lib`
- `cargo test tui --lib`
- `cargo fmt --check`
- `cargo check`
- `git diff --check`

## Remaining

This guards registry-name discoverability. It does not replace focused behavior
tests for command arguments, side effects, or platform/runtime boundaries.
