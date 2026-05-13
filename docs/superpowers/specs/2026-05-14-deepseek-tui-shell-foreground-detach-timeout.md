# DeepSeek-TUI Foreground Shell Detach Timeout Slice

## Context

DeepSeek-TUI can keep foreground shell work controllable: a running foreground
`exec_shell` can be moved into the background and then polled or cancelled. In
DeepSeekCode, `exec_shell` already exposed `timeout_ms` in the model schema, but
the foreground path delegated directly to `run_shell` and ignored that value.

## Implemented

- `exec_shell timeout_ms=<n>` and `exec_shell detach_after_ms=<n>` now run the
  foreground command through the same background job table used by
  `background=true`.
- If the command exits before the timeout, the tool returns a completed shell
  snapshot with `meta.backgrounded=false`.
- If the command is still running, the tool returns `meta.backgrounded=true`, a
  `task_id`, and guidance to poll with `exec_shell_wait` or cancel with
  `exec_shell_cancel`.
- Default foreground `exec_shell` behavior stays unchanged when no timeout is
  supplied.
- Tool schemas and runtime docs now describe the foreground detach timeout.

## Verification

- `cargo test exec_shell_foreground_timeout --lib`
- `cargo test exec_shell --lib`
- `cargo test build_tool_specs_include_exec_shell_background_tools --lib`
- `cargo test mcp_tools_list --lib`
- `cargo fmt --check`
- `cargo check`
- `git diff --check`

## Remaining Gap

This is not a live `Ctrl+B` modal and does not provide supervisor-owned PTY
takeover. It gives model/API calls a deterministic escape hatch for foreground
commands that outlive the requested wait window.
