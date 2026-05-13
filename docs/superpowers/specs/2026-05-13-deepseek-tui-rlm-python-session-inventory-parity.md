# DeepSeek-TUI RLM Python Session Inventory Parity Spec

日期：2026-05-13

对比对象：`Hmbown/DeepSeek-TUI`，`main` HEAD `3382242`

## 背景

`rlm_python_session` 已经能用显式 `session_id` 持久化 JSON state，弥补了
DeepSeek-TUI RLM Python REPL 的跨调用状态能力。但当前 state 只能被下一次执行隐式
读取，模型无法在不运行代码的情况下发现已有 session、检查缓存内容，或判断某个
session 是否存在。

## 目标

- 新增只读 `rlm_python_sessions` tool。
- 不传 `session_id` 时列出 `.dscode/rlm-python/*.json` 中的安全 session。
- 传 `session_id` 时读取指定 session 的 state，并标明文件是否存在。
- 列表模式返回每个 session 的 `session_id`、state 文件路径、字节数和 JSON object
  state。
- 支持 `limit`，默认 20，限制在 1-100。
- 坏 JSON state 不应隐藏；列表模式用 `errors` 返回具体 session 错误。

## 非目标

- 不修改、删除或重置任何 session。
- 不实现长驻 Python 进程或 REPL UI。
- 不读取 `.dscode/rlm-python/` 之外的路径。

## 验收标准

1. registry 在 RLM family depth gate 内暴露 `rlm_python_sessions`。
2. model tool schema 包含 `rlm_python_sessions` 的 `session_id` 和 `limit` 参数。
3. 列表模式能返回已有 session state。
4. 指定 `session_id` 能返回存在/不存在状态和 JSON object state。
5. 非安全 `session_id` 会被拒绝。

## 实现结果

- `src/tools/rlm.rs` 新增 `RlmPythonSessionsTool`。
- 列表模式读取 `.dscode/rlm-python/*.json`，跳过非 JSON/非安全文件名，按
  `session_id` 排序并返回 `sessions` 与 `errors`。
- 单 session 模式复用 `rlm_python_session` 的安全 id 和 JSON object state 校验，返回
  `exists`、`bytes`、`path` 和 `state`。
- `limit` 默认 20，并 clamp 到 1-100。
- registry 和 OpenAI/Anthropic static tool schema 暴露 `rlm_python_sessions`。
- `docs/runtime.md` 和 DeepSeek-TUI parity plan 已补充 session inventory 说明。

## 验证

- `/home/willamhou/.cargo/bin/cargo fmt --check`
- `/home/willamhou/.cargo/bin/cargo test rlm_python_sessions`
- `/home/willamhou/.cargo/bin/cargo test build_tool_specs_include_rlm`
- `/home/willamhou/.cargo/bin/cargo test default_registry_includes_dispatch_subagent_only_below_max_depth`
- `/home/willamhou/.cargo/bin/cargo test rlm`
- `git diff --check`
- `/home/willamhou/.cargo/bin/cargo test`
- `/home/willamhou/.cargo/bin/cargo package --allow-dirty`
