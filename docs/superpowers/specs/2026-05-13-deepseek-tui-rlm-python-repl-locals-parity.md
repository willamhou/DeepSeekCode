# DeepSeek-TUI RLM Python REPL Locals Parity Spec

日期：2026-05-13

对比对象：`Hmbown/DeepSeek-TUI`，`main` HEAD `3382242`

## 背景

DeepSeekCode already has `rlm_python_session`, but callers must explicitly use
`state`, `repl_get`, or `repl_set` to carry values across calls. DeepSeek-TUI's
RLM Python workflow behaves more like a REPL: variables created in one cell are
available in later cells.

## 目标

- Preload safe JSON-object state keys as Python locals for
  `rlm_python_session`.
- Persist JSON-serializable user locals back into the session state after each
  call.
- Preserve explicit `state` dictionary access and non-identifier state keys.
- Keep helper names and private names out of automatic local injection.
- Keep existing sandbox restrictions, timeout handling, and `reset=true`
  behavior.

## 非目标

- This slice does not keep a Python OS process alive between calls.
- This slice does not permit imports, file access, network access, subprocesses,
  or unrestricted Python builtins.
- This slice does not add multi-user session locking beyond the existing file
  state write.

## 验收标准

1. A variable assigned in one `rlm_python_session` call is available by name in
   a later call with the same `session_id`.
2. `reset=true` clears auto-persisted locals.
3. Helper/private names are not injected from state as locals.
4. Existing `state` / `repl_get` / `repl_set` behavior continues to work.
5. Runtime docs and parity plan describe REPL-like local persistence and the
   remaining non-long-lived-process boundary.

## 实现结果

- `rlm_python_session` now injects safe identifier-shaped JSON state keys as
  locals before running the next helper script.
- JSON-serializable user locals are persisted back into the session state after
  each call, while helper names and private `_` names stay out of automatic
  local injection.
- `reset=true` still starts from an empty state before running the code.
- Existing `state`, `SHOW_VARS`, `repl_get`, `repl_set`, `FINAL`, and
  `FINAL_VAR` helper behavior remains covered.
- Runtime docs, model schema text, and the DeepSeek-TUI parity plan now mention
  REPL-like local persistence while leaving true long-lived Python OS processes
  as the remaining boundary.

## 验证

- `cargo test rlm_python_session_auto_persists_safe_locals_when_python_is_available`:
  passed.
- `cargo test rlm_python_repl_helpers_surface_vars_final_and_state_when_python_is_available`:
  passed.
- `cargo test rlm_python_session`: passed, 5 tests.
- `cargo test rlm_python`: passed, 12 tests.
- `cargo test build_tool_specs_include_rlm`: passed.
- `cargo test`: passed, 1029 tests.
- `cargo fmt --check`: passed.
- `git diff --check`: passed.
- `cargo package --allow-dirty`: passed, packaged 306 files.
