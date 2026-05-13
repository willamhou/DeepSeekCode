# DeepSeek-TUI TUI Slash Completion

Status: implemented

## Gap

DeepSeek-TUI exposes slash-command discovery from the composer: typing `/...`
shows matching built-in commands and `Tab` can complete slash command prefixes.
DeepSeekCode already executed several composer slash commands, but users had to
know the command names up front or open the command palette separately.

## Implementation

- Added a composer-specific built-in slash completion catalog covering local
  slash commands that the composer can execute without starting a model turn.
- Merged project `.dscode/commands/**/*.md` custom slash commands into the
  focused composer hint list and `Tab` completion path.
- Injected configured user custom commands and configured skill names as
  `/skill <name>` entries for local file-backed TUI sessions.
- Added composer `Tab` completion for `/...` prefixes using the same longest
  common prefix behavior as the command palette.
- Rendered a dim slash hint line under the composer while the focused composer
  starts with `/`, including bounded candidate previews and a remaining-count
  indicator.
- Kept command-palette completions separate so palette-only commands are not
  advertised as composer slash commands.
- Updated TUI documentation and the DeepSeek-TUI parity plan.

## Verification

- `cargo test composer_slash_tab_completes_and_renders_hints --lib`
- `cargo test composer_slash_hints_include_project_custom_commands --lib`
- `cargo test composer_slash_hints_include_extra_configured_entries --lib`
- `cargo test configure_tui_slash_completions_includes_user_commands_and_skills --lib`
- `cargo test composer_intercepts_memory_prefix_and_slash_commands --lib`
- `cargo test tui --lib`
- `cargo fmt --check`
- `cargo check`
- `git diff --check`

## Remaining

DeepSeekCode's slash hints now include built-ins, project custom commands,
configured user custom commands, and configured skill names. HTTP-runtime TUI
sessions still show only local app-level completions because they do not carry
the file-backed workspace config.
