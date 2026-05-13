# DeepSeek-TUI TUI Shell Supervisor Status

Status: implemented

## Gap

DeepSeek Code now has a workspace-local shell supervisor protocol skeleton and
`exec_shell_supervisor_status`, but the local TUI command palette only exposed
shell job actions. DeepSeek-TUI-style terminal workbench users need an in-TUI
read-only way to inspect the supervisor manifest, socket, and protocol health
without dropping back to the CLI.

## Implementation

- Added a `ShellSupervisorStatus` TUI action and command-palette routes for
  `shell supervisor`, `shell supervisor-status`, `shell status`,
  `sh supervisor`, `sh supervisor-status`, `jobs supervisor`, and
  `jobs supervisor-status`.
- Wired the TUI action handler to `ExecShellSupervisorStatusTool`, rendering
  successful output in the shell detail panel and surfacing failures in the
  status line.
- Added completion entries for `shell supervisor` and `jobs supervisor`.
- Updated TUI/runtime documentation and the DeepSeek-TUI parity plan to include
  the shell supervisor status surface.

## Verification

- `cargo test command_palette_requests_shell_job_actions --lib`
- `cargo test handle_tui_http_action_rejects_shell_commands_as_local_only --lib`
- `cargo test tui --lib`
- `cargo fmt --check`
- `cargo check`
- `git diff --check`

## Remaining

This is a read-only protocol/status surface. Native supervisor-owned PTY
sessions, attachable terminal takeover after owner-process exit, and real resize
control remain tracked by the shell-supervisor/PTY design spec.
