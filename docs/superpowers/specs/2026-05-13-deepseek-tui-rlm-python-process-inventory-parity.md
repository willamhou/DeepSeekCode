# DeepSeek-TUI RLM Python Process Inventory Parity

## Gap

`rlm_python_session persistent=true` can now keep a Python REPL process alive,
but `rlm_python_sessions` only reported file-backed JSON state. That leaves the
model unable to tell whether a session has an active in-process REPL before
deciding to continue, reset, or inspect it.

## Target

- Add process metadata to `rlm_python_sessions` outputs.
- Report `process.active` and `process.pid` for persistent Python REPLs alive
  in the current DeepSeekCode process.
- Keep inventory read-only and avoid spawning Python while listing sessions.
- Remove stale process entries when inventory observes an exited child.

## Acceptance Criteria

1. Single-session inventory includes `process.active=false` when no persistent
   REPL is active.
2. After `rlm_python_session persistent=true`, single-session inventory reports
   `process.active=true` and the live Python PID.
3. List inventory includes the same `process` object for each persisted session.
4. Existing state-file listing behavior remains unchanged.
5. Schema/docs describe process inventory.
6. Focused tests and full Rust gates pass.

## Verification

- `/home/willamhou/.cargo/bin/cargo test rlm_python_session_persistent_process_reuses_python_pid_when_available`
- `/home/willamhou/.cargo/bin/cargo test rlm_python_sessions`
- `/home/willamhou/.cargo/bin/cargo test build_tool_specs_include_rlm`
- `/home/willamhou/.cargo/bin/cargo test`
- `/home/willamhou/.cargo/bin/cargo fmt --check`
- `git diff --check`
- `/home/willamhou/.cargo/bin/cargo package --allow-dirty`
