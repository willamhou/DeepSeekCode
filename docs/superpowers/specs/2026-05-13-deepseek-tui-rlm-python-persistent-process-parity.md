# DeepSeek-TUI RLM Python Persistent Process Parity

## Gap

DeepSeekCode already has `rlm_python_session` with file-backed JSON state and
REPL-like locals, but each call still starts a fresh Python interpreter. That is
materially weaker than DeepSeek-TUI-style RLM helper loops for incremental
analysis because process-local variables and interpreter warm state cannot be
reused inside a running CLI process.

## Target

- Add optional `persistent=true` to `rlm_python_session`.
- Reuse one restricted Python REPL process per `session_id` and config directory
  while DeepSeekCode is running.
- Keep the existing JSON state file as the durable source of truth across
  process restarts.
- Make `reset=true` clear state and rebuild the cached Python process.

## Acceptance Criteria

1. Non-persistent `rlm_python_session` behavior stays unchanged.
2. `persistent=true` returns the same Python PID on repeated calls to the same
   `session_id` within one DeepSeekCode process.
3. Persistent sessions still write `.dscode/rlm-python/<session_id>.json`.
4. `reset=true` clears file-backed state and closes/rebuilds the process cache.
5. Tool schema and runtime docs describe `persistent`.
6. Focused tests and full Rust gates pass.

## Implementation Notes

- `RLM_PYTHON_REPL_SANDBOX` uses a JSON-lines protocol over stdin/stdout.
- Rust caches child processes behind a `OnceLock<Mutex<HashMap<...>>>`.
- The one-shot sandbox remains the default for conservative compatibility.

## Verification

- `/home/willamhou/.cargo/bin/cargo test rlm_python_session_persistent_process_reuses_python_pid_when_available`
- `/home/willamhou/.cargo/bin/cargo test rlm_python`
- `/home/willamhou/.cargo/bin/cargo test build_tool_specs_include_rlm`
- `/home/willamhou/.cargo/bin/cargo test`
- `/home/willamhou/.cargo/bin/cargo fmt --check`
- `git diff --check`
- `/home/willamhou/.cargo/bin/cargo package --allow-dirty`
