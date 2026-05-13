# DeepSeek-TUI TUI Rollback Apply Confirmation

## Context

DeepSeekCode's TUI rollback commands can create snapshots, list snapshots, show
snapshot details, dry-run restore plans, and apply a restore through
`revert turn <id|last> --apply`. The apply path mutates the local git worktree,
but before this slice the command palette queued the write action immediately.

DeepSeek-TUI parity should make destructive local restore actions explicit and
hard to trigger accidentally.

## Scope

- Route `revert turn <id|last> --apply` and equivalent restore aliases through
  a TUI confirmation modal.
- Pressing `y` or Enter confirms and queues the existing local
  `TuiAction::RevertTurn { apply: true }` action.
- Pressing `n` or Esc cancels without queuing any rollback write action.
- Keep dry-run rollback unchanged: `revert turn <id|last>` still queues an
  immediate dry-run action and renders the restore plan through the existing
  right-side panel.

## Non-Goals

- Interactive per-hunk restore selection.
- Changing CLI or MCP rollback approval semantics.
- Remote HTTP-runtime rollback support.

## Verification

- `cargo test command_palette_confirms_rollback_apply_before_queueing --lib`
- `cargo test command_palette_cancels_rollback_apply_confirmation --lib`
- `cargo test command_palette_requests_rollback_show_and_revert_last_turn --lib`
- `cargo test handle_tui_action_lists_shows_and_restores_rollback_snapshot --lib`
- `cargo test tui --lib`
- `cargo fmt --check`
- `git diff --check`

## Remaining Differences

- Diff hunk browsing and selective restore remain future Phase F work.
