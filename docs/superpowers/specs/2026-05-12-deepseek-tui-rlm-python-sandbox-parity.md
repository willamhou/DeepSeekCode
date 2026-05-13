# DeepSeek-TUI RLM Python Sandbox Parity Spec

日期：2026-05-12

对比对象：`Hmbown/DeepSeek-TUI`，`main` HEAD `3382242`

## 背景

DeepSeekCode 已有 `rlm` / `rlm_batch` / `llm_query*` 这些 RLM-lite child-agent
入口，但 DeepSeek-TUI 的完整 RLM 还包含 Python REPL sandbox，让模型可以用短脚本做
分块、聚合、分类和中间计算。当前 DeepSeekCode 缺少任何 Python helper runtime。

## 目标

- 新增 `rlm_python` tool，执行短 Python 片段做纯计算和文本分析。
- 输入可带 `context` / `question`，Python 环境中直接暴露同名变量。
- 限制代码长度、危险 token、执行超时和 stdout 大小。
- Python 环境只暴露少量 safe builtins 与 `math` / `statistics` / `re` /
  `Counter` / `defaultdict`。
- 接入 registry 和 OpenAI/Anthropic static tool schema。

## 非目标

- 不实现长期交互式 REPL session。
- 不提供文件、网络、subprocess、import 或任意 OS 访问。
- 不声称这是对恶意 Python 的强安全沙箱；本轮是模型自用的受限 helper runtime。

## 验收标准

1. registry 在 RLM family depth gate 内暴露 `rlm_python`。
2. model tool schema 包含 `rlm_python` 的 `code`、`context`、`question`、
   `timeout_ms` 参数。
3. `rlm_python` 能执行小型纯计算脚本并返回 stdout / variables JSON。
4. `rlm_python` 拒绝 import / open / dunder 等危险 token。
5. `rlm_python` 超时会杀掉 Python 子进程并返回失败。

## 实现结果

- `src/tools/rlm.rs` 新增 `RlmPythonTool`。
- `rlm_python` 通过 `python3 -I -S` 启动短生命周期 helper，并通过 stdin 传入
  `code` / `context` / `question`。
- helper 环境只暴露 safe builtins、`math`、`statistics`、`re`、`Counter`、
  `defaultdict`、`context` 和 `question`。
- Rust 侧和 Python wrapper 都阻止 import/file/network/subprocess/dunder 等危险 token。
- Rust 侧等待子进程时执行 timeout kill，默认 2000ms，允许 100-5000ms clamp。
- registry 在 RLM family depth gate 内注册 `rlm_python`；OpenAI/Anthropic static
  tool schema 暴露 `code`、`context`、`question`、`timeout_ms`。

## 验证

- `cargo fmt --check`
- `cargo test rlm`
- `cargo test build_tool_specs_include_rlm`
- `cargo test default_registry_includes_dispatch_subagent_only_below_max_depth`
- `git diff --check`
- `cargo test`
- `cargo package --allow-dirty`
