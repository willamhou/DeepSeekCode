# DeepSeek-TUI TUI Command History Parity Spec

Date: 2026-05-13

Source comparison: `Hmbown/DeepSeek-TUI`, `main` HEAD `3382242`

## Gap

DeepSeekCode's command palette could execute local workbench commands, but each
open started from a blank input with no way to recall a previous command. That
kept repeated TUI workflows slower than a terminal workbench should be,
especially for MCP manager, diagnostics, task, automation, and rollback
commands.

## Scope

- Keep a bounded in-memory command-palette history for the current TUI process.
- Record non-empty commands on execution without duplicating consecutive
  identical entries.
- Use `Up` / `Down` while the command palette is active to recall older/newer
  commands.
- Preserve and restore the in-progress draft when navigating back to the live
  input after browsing history.
- Document the key behavior and update the DeepSeek-TUI parity plan.

## Acceptance

1. Executed command-palette commands are recorded in order.
2. `Up` recalls the most recent command, then older commands.
3. `Down` moves toward newer commands and restores the draft after the newest
   history entry.
4. The command history is bounded.
5. The user-facing TUI docs and parity plan mention the behavior.

## Implementation Notes

- Added `command_history`, `command_history_index`, and
  `command_history_draft` to `TuiApp`.
- Added `record_command_history` and `navigate_command_history`.
- Wired `Up` / `Down` in `handle_command_palette_key`.

## Verification

- `/home/willamhou/.cargo/bin/cargo test command_palette_browses_history_with_up_and_down`
- `/home/willamhou/.cargo/bin/cargo test tui`
- `/home/willamhou/.cargo/bin/cargo fmt --check`
- `git diff --check`
- `/home/willamhou/.cargo/bin/cargo test`
- `/home/willamhou/.cargo/bin/cargo package --allow-dirty`
