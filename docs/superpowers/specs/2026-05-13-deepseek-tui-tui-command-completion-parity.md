# DeepSeek-TUI TUI Command Completion Parity Spec

Date: 2026-05-13

Source comparison: `Hmbown/DeepSeek-TUI`, `main` HEAD `3382242`

## Gap

DeepSeekCode's command palette now supports editing and command history, but it
still required exact command recall or manual typing. DeepSeek-TUI-style terminal
workbenches need a faster command entry path for repeated MCP, diagnostics,
rollback, task, automation, and mode commands.

## Scope

- Add `Tab` completion while the command palette is active.
- Complete unique built-in command prefixes directly.
- Expand ambiguous prefixes to the longest common prefix when possible.
- If no further common prefix exists, leave input unchanged and show candidate
  hints in the status bar.
- Keep global `Tab` mode cycling unchanged when the command palette is not
  active.
- Document the key behavior and update the DeepSeek-TUI parity plan.

## Acceptance

1. `mode a` + `Tab` completes to `mode agent`.
2. Ambiguous prefixes such as `mcp man` expand to the common command prefix.
3. Ambiguous exact prefixes report candidate hints instead of executing a
   command.
4. Existing command-palette execution and global mode cycling still pass TUI
   tests.

## Implementation Notes

- Added a bounded static command completion catalog for current palette
  commands.
- Added `complete_command_palette` plus a small longest-common-prefix helper.
- Wired `KeyCode::Tab` inside `handle_command_palette_key`; global Tab behavior
  remains in the main key handler.

## Verification

- `/home/willamhou/.cargo/bin/cargo test command_palette_tab_completes_unique_prefix`
- `/home/willamhou/.cargo/bin/cargo test command_palette_tab_completes_common_prefix`
- `/home/willamhou/.cargo/bin/cargo test tui`
- `/home/willamhou/.cargo/bin/cargo fmt --check`
- `git diff --check`
- `/home/willamhou/.cargo/bin/cargo test`
- `/home/willamhou/.cargo/bin/cargo package --allow-dirty`
