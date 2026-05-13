# DeepSeek-TUI TUI Shell Job Command Parity Spec

Date: 2026-05-13

Source comparison: `Hmbown/DeepSeek-TUI`, `main` HEAD `3382242`

## Gap

DeepSeek-TUI exposes a foreground shell job center through slash commands such
as `/jobs poll`, `/jobs wait`, `/jobs stdin`, and `/jobs cancel`, and routes
those actions through its process-local shell manager. DeepSeekCode already had
agent-visible `exec_shell` / `exec_shell_wait` / `exec_shell_cancel`, but the
TUI command palette could not start, inspect, or cancel shell jobs directly.

## Scope

- Add TUI command-palette actions for safe local background shell jobs.
- Reuse the existing `exec_shell` allowlist and process-local job manager.
- Start jobs with `shell <command>`, `shell run <command>`, or `! <command>`.
- Poll/wait with `shell poll <id>` and `shell wait <id> [timeout_ms]`.
- Cancel with `shell cancel <id|all>`.
- Add DeepSeek-TUI-style `jobs poll|wait|cancel` aliases.
- Render shell job start/poll/cancel details in the right-side task panel.
- Keep HTTP runtime mode explicit: shell jobs require local file-backed TUI.
- Document the command surface and update the parity plan.

## Acceptance

1. `shell <command>` and `! <command>` enqueue a `RunShell` TUI action.
2. `shell wait`, `shell poll`, and `shell cancel` enqueue typed shell job
   actions with task ids and timeout metadata.
3. `jobs poll|wait|cancel` aliases map to the same actions.
4. Local TUI action handling starts allowlisted commands through `exec_shell`
   in background mode and shows the task id.
5. Local TUI action handling can wait/poll a shell job and render output deltas.
6. Local TUI action handling can cancel one or all shell jobs.
7. Remote HTTP TUI reports shell commands as local-only instead of silently
   ignoring them.
8. Existing TUI command and handler tests remain green.

## Implementation Notes

- Added `RunShell`, `WaitShell`, and `CancelShell` to `TuiAction`.
- Added a `Shell Jobs` right-side detail panel kind.
- Added command-palette parsing for `shell`, `sh`, `!`, and `jobs` aliases.
- Wired local action handling to `ExecShellTool`, `ExecShellWaitTool`, and
  `ExecShellCancelTool`.
- Kept execution behind the existing safe-shell allowlist rather than adding a
  new arbitrary command path.

## Verification

- `/home/willamhou/.cargo/bin/cargo test shell_job --lib`
- `/home/willamhou/.cargo/bin/cargo test shell --lib`
- `/home/willamhou/.cargo/bin/cargo test tui --lib`
- `/home/willamhou/.cargo/bin/cargo fmt --check`
- `git diff --check`
- `/home/willamhou/.cargo/bin/cargo test`
- `/home/willamhou/.cargo/bin/cargo package --allow-dirty`
