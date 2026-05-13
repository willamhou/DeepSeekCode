# DeepSeek-TUI TUI Custom Slash Commands

Status: implemented

## Gap

DeepSeek-TUI exposes user-defined slash commands in the terminal workflow. Before
this slice, DeepSeekCode could load project and user markdown slash commands
from the REPL, but the local TUI composer and command palette treated unknown
slash-prefixed input as ordinary text or unknown palette commands.

## Implementation

- Shared the REPL custom markdown command loader behind a config-based helper so
  the TUI uses the same project/user lookup order, frontmatter stripping, and
  argument expansion semantics.
- Added a `RunCustomSlashCommand` TUI action for slash-prefixed composer and
  command-palette input.
- Wired the local file-backed TUI action handler to expand the markdown prompt,
  append it as a durable user turn/item, and start the active-thread background
  agent run.
- Kept HTTP-runtime TUI behavior explicit: custom slash commands require local
  file-backed config and command markdown files.
- Updated TUI, REPL, and DeepSeek-TUI parity documentation.

## Verification

- `cargo test command_palette_requests_custom_slash_command --lib`
- `cargo test composer_intercepts_memory_prefix_and_slash_commands --lib`
- `cargo test handle_tui_action_reports_missing_custom_slash_command --lib`
- `cargo test handle_tui_http_action_rejects_custom_slash_commands_as_local_only --lib`
- `cargo test tui --lib`
- `cargo fmt --check`
- `cargo check`
- `git diff --check`

## Remaining

Dynamic discovery/completion of custom command names is not implemented yet; the
command palette still completes only built-in workbench commands.
