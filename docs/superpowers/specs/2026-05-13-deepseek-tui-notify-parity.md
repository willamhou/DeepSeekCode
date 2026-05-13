# DeepSeek-TUI Notify Parity Spec

日期：2026-05-13

对比对象：`Hmbown/DeepSeek-TUI`，`main` HEAD `3382242`

## 背景

DeepSeek-TUI exposes a read-only `notify` tool so the model can fire a single
terminal/desktop attention signal when a long-running task completes or the user
needs to return to the terminal.

DeepSeekCode does not currently expose an agent-callable notification tool.

## 目标

- Add read-only `notify`.
- Require a short `title`; accept optional `body`.
- Bound title and body length by Unicode scalar count.
- Keep the tool approval-free and safe.
- Provide a terminal-bell implementation that can be disabled with
  `DSCODE_NOTIFY=off` for quiet environments/tests.
- Expose the tool through model schemas and the default registry.

## 非目标

- This slice does not add platform-native notification APIs.
- This slice does not add notification configuration files.
- This slice does not emit progress notifications automatically.

## 验收标准

1. `notify title=<text>` validates and returns a bounded confirmation.
2. Empty or whitespace-only title is rejected.
3. Long titles/bodies are truncated safely.
4. Registry and model schemas include `notify`.
5. The tool is read-only and does not require approval.

## 实现结果

- Added `src/tools/notify.rs` with agent-visible `notify`.
- `title` is required and rejected when empty or whitespace-only.
- `body` is optional.
- `title` is truncated to 80 Unicode scalar values; `body` is truncated to 200
  Unicode scalar values.
- The tool emits a terminal bell to stderr by default and is silent when
  `DSCODE_NOTIFY=off`, `0`, or `false`.
- The tool is read-only and approval-free in the default registry.
- Registered `notify` in the runtime tool registry and static model tool
  schemas.
- Documented the DeepSeek-TUI-compatible notification tool in
  `docs/runtime.md` and the parity plan.

## 验证

- `/home/willamhou/.cargo/bin/cargo test notify` passed: 6 tests.
- `/home/willamhou/.cargo/bin/cargo test default_registry_includes_read_only_git_history_tools` passed.
- `/home/willamhou/.cargo/bin/cargo fmt --check` passed.
- `git diff --check` passed.
- `/home/willamhou/.cargo/bin/cargo test` passed: 946 tests.
- `/home/willamhou/.cargo/bin/cargo package --allow-dirty` passed: packaged
  277 files and verified `deepseek_code v0.1.0`.
