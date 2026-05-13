# DeepSeek-TUI Run Tests Parity Spec

日期：2026-05-13

对比对象：`Hmbown/DeepSeek-TUI`，`main` HEAD `3382242`

## 背景

DeepSeek-TUI exposes `run_tests` as a first-class code-executing test runner.
DeepSeekCode could already run safe test commands through `run_shell`, but lacked
the direct tool name and schema that models expect when translating from
DeepSeek-TUI workflows.

## 目标

- Add `run_tests` as a first-class agent tool.
- Infer common test commands from project files when `command` is omitted.
- Accept only supported test command prefixes and safe argument tails.
- Reuse existing `run_shell` execution, metadata, cancellation, and shell
  approval semantics.
- Expose `run_tests` in model schemas and as a gated MCP/ACP side-effect tool.

## 非目标

- This slice does not add background shell wait/interact/cancel tools.
- This slice does not support arbitrary shell syntax in `args`.
- This slice does not add test result artifact storage.

## 验收标准

1. Default registry exposes `run_tests`.
2. `run_tests` produces the same test metadata shape as `run_shell`.
3. Unsafe command tails are rejected.
4. OpenAI/Anthropic schemas include `command`, `args`, and `all_features`.
5. Registry permission requests classify `run_tests` as shell execution.
6. MCP/ACP only expose `run_tests` when side effects or durable approvals are
   enabled.

## 实现结果

- `src/tools/run_tests.rs` adds `RunTestsTool`.
- `src/tools/registry.rs` registers it and routes permission requests through
  shell approval semantics.
- `src/model/deepseek.rs` exposes the schema.
- `src/cli/commands/serve.rs` exposes it as a gated MCP/ACP side-effect tool.
- `docs/runtime.md` and the DeepSeek-TUI parity plan document the new tool.

## 验证

- `/home/willamhou/.cargo/bin/cargo fmt --check`
- `/home/willamhou/.cargo/bin/cargo test run_tests`
- `/home/willamhou/.cargo/bin/cargo test mcp_tools_list_includes_workspace_and_runtime_tools`
- `/home/willamhou/.cargo/bin/cargo test mcp_tools_list_includes_run_shell_when_side_effects_enabled`
- `/home/willamhou/.cargo/bin/cargo test acp_session_tools_list_new_session_is_read_only`
- `/home/willamhou/.cargo/bin/cargo test`
- `git diff --check`
- `/home/willamhou/.cargo/bin/cargo package --allow-dirty`
