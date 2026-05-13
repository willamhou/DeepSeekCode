# DeepSeek-TUI RLM Python Stateful Session Parity Spec

日期：2026-05-13

对比对象：`Hmbown/DeepSeek-TUI`，`main` HEAD `3382242`

## 背景

上一片已经新增 `rlm_python`，可以执行短生命周期受限 Python helper。DeepSeek-TUI 的
RLM Python REPL 仍有一个关键优势：同一个 REPL 会话能跨多次 helper 调用保留中间
状态。DeepSeekCode 当前每次 `rlm_python` 都是无状态执行，模型无法逐步构建分块索引、
分类缓存或聚合计数。

## 目标

- 新增 `rlm_python_session` tool。
- 通过显式 `session_id` 持久化 JSON state 到 workspace config dir。
- Python 环境暴露可读写的 `state` 字典，以及 `context` / `question`。
- 成功执行后保存 JSON-serializable `state`，下一次同 session 自动加载。
- 支持 `reset=true` 以清空指定 session state 后再执行。
- 沿用 `rlm_python` 的代码长度、危险 token、timeout 和 stdout 限制。

## 非目标

- 不实现长驻 Python 进程。
- 不保存任意 Python object，只保存 JSON object state。
- 不允许文件、网络、subprocess、import 或 OS 访问。

## 验收标准

1. registry 在 RLM family depth gate 内暴露 `rlm_python_session`。
2. model tool schema 包含 `rlm_python_session` 的 `session_id`、`code`、
   `context`、`question`、`timeout_ms`、`reset` 参数。
3. 两次同 `session_id` 调用可以共享并更新 `state`。
4. `reset=true` 会清空旧 state。
5. 非安全 `session_id` 和危险 Python token 会被拒绝。

## 实现结果

- `src/tools/rlm.rs` 新增 `RlmPythonSessionTool`。
- `rlm_python_session` 使用 `.dscode/rlm-python/<session_id>.json` 保存 JSON object
  state。
- Python helper 环境现在暴露 `state` 字典；脚本成功执行后 Rust 侧解析输出并保存
  `state`。
- `session_id` 只接受 1-64 个 `[A-Za-z0-9_.-]` 字符，拒绝 leading dot 和 `..`。
- `reset=true` / `1` / `yes` / `on` 会从空 state 开始执行并覆盖旧 state。
- registry 和 OpenAI/Anthropic static tool schema 暴露 `rlm_python_session`。

## 验证

- `cargo fmt --check`
- `cargo test rlm_python`
- `cargo test build_tool_specs_include_rlm`
- `cargo test default_registry_includes_dispatch_subagent_only_below_max_depth`
- `cargo test rlm`
- `git diff --check`
- `cargo test`
- `cargo test mcp_tools_call_rejects_write_file_after_runtime_denial`
- `cargo package --allow-dirty`
