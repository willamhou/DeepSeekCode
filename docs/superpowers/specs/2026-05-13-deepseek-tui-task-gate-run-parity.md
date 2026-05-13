# DeepSeek-TUI Task Gate Run Parity Spec

日期：2026-05-13

对比对象：`Hmbown/DeepSeek-TUI`，`main` HEAD `3382242`

## 背景

DeepSeek-TUI exposes `task_gate_run` for verification gates such as fmt, check,
clippy, test, or custom commands. DeepSeekCode already has a safe shell runner
and `run_tests`, but not the DeepSeek-TUI-compatible gate tool name.

## 目标

- Add agent-visible `task_gate_run`.
- Accept required `gate` and `command`, plus optional `cwd` and `timeout_ms`
  compatibility field.
- Reuse the existing safe `run_shell` execution path and cancellation support.
- Require shell approval through the existing shell permission path.
- Return structured gate metadata plus the existing shell summary.

## 非目标

- This slice does not persist gate artifacts onto runtime tasks.
- This slice does not implement independent timeout enforcement beyond existing
  cancellation support.
- This slice does not add PR-attempt tools.

## 验收标准

1. `task_gate_run gate=<kind> command=<cmd>` validates gate and command.
2. Supported gate kinds are `fmt`, `check`, `clippy`, `test`, and `custom`.
3. Unsafe commands are rejected by the existing shell safety check.
4. The registry exposes `task_gate_run`.
5. The model schema exposes `task_gate_run`.
6. Permission requests classify `task_gate_run` as a shell action with the
   command target.

## 实现结果

- Added `TaskGateRunTool` in `src/tools/runtime_tasks.rs`.
- `task_gate_run` accepts `gate`, `command`, optional `cwd`, and optional
  `timeout_ms`.
- Gate validation accepts `fmt`, `check`, `clippy`, `test`, and `custom`, and
  rejects unknown gate labels before command execution.
- Execution delegates to the existing `RunShellTool::execute_with_cancel`
  implementation so command allow-listing, unsafe command rejection, cwd
  handling, and cancellation behavior stay shared with `run_shell`.
- The output prepends gate metadata (`meta.gate`, `meta.command`, and optional
  `meta.timeout_ms`) before the existing shell summary.
- Registered `task_gate_run` in the default tool registry.
- Routed `task_gate_run` through the existing shell permission request and
  direct confirmation paths with the command as the approval target.
- Added the static DeepSeek model schema entry for `task_gate_run`.
- Updated runtime docs and the DeepSeek-TUI parity plan to include the gate
  runner.

## 验证

- `cargo test task_gate_run`: passed, 2 tests.
- `cargo test build_tool_specs_include_runtime_task_and_automation_tools`:
  passed.
- `cargo test permission_request_for_reports_write_shell_and_mcp_prompts`:
  passed.
- `cargo fmt --check`: passed.
- `git diff --check`: passed.
- `cargo test`: passed, 966 tests.
- `cargo package --allow-dirty`: passed, packaged 283 files and verified
  `deepseek_code v0.1.0`.
