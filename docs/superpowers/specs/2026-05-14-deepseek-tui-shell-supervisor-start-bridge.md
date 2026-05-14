# DeepSeek-TUI parity: shell supervisor control bridge

Status: implemented
Date: 2026-05-14

## Gap

The workspace shell supervisor socket could report health/status and durable job
inventory, and it could start safe background shell jobs, but clients still had
to call model tools directly for the rest of the shell lifecycle. That left the
supervisor protocol short of the control surface needed by a TUI or external
runtime client.

## Implementation

- `deepseek agents shell-supervisor --json` now advertises
  `start`, `wait`, `replay`, `attach`, `stdin`, `resize`, and `cancel` as
  supported newline-JSON methods.
- `{"method":"start","command":"..."}` and
  `{"method":"start","arguments":{"command":"..."}}` create a durable
  `task_shell_start` background job owned by the supervisor process.
- `start` accepts safe shell commands plus optional workspace-contained `cwd`,
  `stdin`, `tty`, `tty_rows`, `tty_cols`, and scalar `env` fields.
- The response returns `task_id`, `start_summary`, `job_tty`,
  `job_pty_backend`, and a refreshed `active_jobs` count.
- `wait`, `replay`, `attach`, `stdin`, `resize`, and `cancel` bridge to the
  existing durable shell tools and return method-specific summary fields.
- Control requests refresh the supervisor manifest `active_jobs` count after
  they run, so TUI clients can keep their shell inventory current from the
  socket response.
- The supervisor manifest now lists the full control method set in `methods`
  and leaves `unsupported_methods` empty. This is a durable shell bridge, not a
  claim that native supervisor-owned PTYs are complete.

## Verification

- `cargo test shell_supervisor_protocol_start_creates_durable_job --lib`
- `cargo test shell_supervisor_protocol_controls_durable_jobs --lib`
- `cargo test shell_supervisor --lib`
- `cargo test exec_shell_supervisor_status --lib`
- `cargo check`
- `cargo fmt --check`
- `cargo test --lib -- --test-threads=1`
- `git diff --check`

## Remaining Gap

This is a supervisor-owned durable shell bridge over the existing
plain-pipe/`script` backends. Native supervisor-owned PTY takeover, live
terminal event streams, and OS-level PTY resize ownership remain open.
