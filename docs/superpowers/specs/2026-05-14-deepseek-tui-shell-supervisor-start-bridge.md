# DeepSeek-TUI parity: shell supervisor start bridge

Status: implemented
Date: 2026-05-14

## Gap

The workspace shell supervisor socket could report health/status and durable job
inventory, but clients still had to call model tools directly to create a
background shell job. That left the supervisor protocol as read-only metadata
instead of a process-owned control surface.

## Implementation

- `deepseek agents shell-supervisor --json` now advertises `start` as a
  supported newline-JSON method.
- `{"method":"start","command":"..."}` and
  `{"method":"start","arguments":{"command":"..."}}` create a durable
  `task_shell_start` background job owned by the supervisor process.
- `start` accepts safe shell commands plus optional workspace-contained `cwd`,
  `stdin`, `tty`, `tty_rows`, `tty_cols`, and scalar `env` fields.
- The response returns `task_id`, `start_summary`, `job_tty`,
  `job_pty_backend`, and a refreshed `active_jobs` count.
- The supervisor manifest now lists `start` in `methods` and no longer lists it
  in `unsupported_methods`.

## Verification

- `cargo test shell_supervisor_protocol_start_creates_durable_job --lib`
- `cargo test shell_supervisor --lib`
- `cargo test exec_shell_supervisor_status --lib`
- `cargo check`
- `cargo fmt --check`
- `cargo test --lib -- --test-threads=1`
- `git diff --check`

## Remaining Gap

This is a supervisor-owned durable shell bridge over the existing
plain-pipe/`script` backends. It still does not implement native
supervisor-owned PTY attach, stdin, resize, replay, wait, or cancel methods.
