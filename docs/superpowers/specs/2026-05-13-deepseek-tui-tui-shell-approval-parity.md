# DeepSeek-TUI TUI Shell Approval Parity

Date: 2026-05-13

## Gap

DeepSeek-TUI-style terminal workbenches can route explicit foreground user
actions through confirmation instead of failing immediately when the command is
outside a conservative allowlist. DeepSeekCode's TUI shell command palette
started allowlisted background jobs, but unallowlisted commands failed at the
tool layer with no foreground approval path.

## Scope

- Keep model-facing `run_shell` / `exec_shell` allowlist enforcement unchanged.
- Detect unallowlisted commands entered by the user through the TUI command
  palette (`shell <command>`, `shell run <command>`, `! <command>`).
- Open the existing approval modal for those foreground shell commands.
- On approval, run the command once as a local background shell job through a
  TUI-only trusted path.
- On denial or close, do not run the command.
- Keep remote HTTP runtime TUI behavior explicit: shell commands remain
  local-only.
- Document the approved foreground shell behavior in `docs/tui.md` and the
  parity plan.

## Acceptance

- Allowlisted TUI shell commands still enqueue normal `RunShell` actions.
- Unallowlisted TUI shell commands enqueue no shell action until the user
  approves the modal.
- Approving the modal enqueues a distinct approved-shell action and starts a
  background job through the local TUI handler.
- Denying the modal does not start a job.
- No model-registered shell tool gains an argument-based allowlist bypass.
- Focused and full Rust test gates pass.

## Verification

- `cargo test command_palette_requires_approval_for_unallowlisted_shell_command --lib`: passed.
- `cargo test handle_tui_action_runs_approved_shell_job --lib`: passed.
- `cargo test shell --lib`: 40 passed.
- `cargo test tui --lib`: 104 passed.
- `cargo fmt --check`: passed.
- `git diff --check`: passed.
- `cargo test`: 1071 passed.
- `cargo package --allow-dirty`: passed.
