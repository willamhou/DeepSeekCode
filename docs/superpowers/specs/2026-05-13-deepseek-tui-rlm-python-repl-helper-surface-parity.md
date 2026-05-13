# DeepSeek-TUI RLM Python REPL Helper Surface Parity Spec

日期：2026-05-13

对比对象：`Hmbown/DeepSeek-TUI`，`main` HEAD `3382242`

## 背景

DeepSeek-TUI 的 RLM REPL 除了 chunk helper 外，还暴露
`SHOW_VARS()`、`repl_get()`、`repl_set()`、`FINAL()` 和 `FINAL_VAR()`。这些名字会出现在
RLM paper/reference prompt 和 DeepSeek-TUI 的示例中。DeepSeekCode 的 Python helper
虽然仍是短生命周期 sandbox，但可以兼容这些 helper 名称，让同一类代码片段更容易迁移。

## 目标

- `SHOW_VARS()` 返回当前用户变量名到类型名的 JSON object。
- `repl_get(name, default=None)` 从当前变量或 `state` 读取值。
- `repl_set(name, value)` 写入当前变量，并同步到 `state` 以支持 session 持久化。
- `FINAL(value)` 写入 `final` 变量并打印结果。
- `FINAL_VAR(name)` 用指定变量调用 `FINAL`。
- 这些 helper 同时适用于 `rlm_python` 和 `rlm_python_session`。

## 非目标

- 不让 `FINAL` 结束外层 RLM turn loop；当前没有完整 RLM loop。
- 不放宽 Python sandbox 的 import/file/network/subprocess 限制。
- 不支持非 JSON-serializable state 持久化。

## 验收标准

1. `rlm_python` 可以调用 `SHOW_VARS`、`repl_set`、`repl_get` 和 `FINAL_VAR`。
2. `repl_set` 写入的值出现在输出 `state` 中。
3. `rlm_python_session` 能跨调用读取 `repl_set` 写入的 persisted state。
4. 原有危险 token、timeout、chunk helper 行为不回归。

## 实现结果

- `RLM_PYTHON_SANDBOX` 注入 `SHOW_VARS`、`repl_get`、`repl_set`、`FINAL`、
  `FINAL_VAR`。
- `SHOW_VARS` 读取调用方 globals，过滤 helper/内置/callable，返回变量类型名。
- `repl_set` 同步写入调用方变量和 `state`，因此 `rlm_python_session` 会持久化它。
- `FINAL` / `FINAL_VAR` 写入 `final` 变量并打印最终值，便于迁移 DeepSeek-TUI
  REPL 片段。
- 移除了 `vars(` 字符串级屏蔽，避免误伤 `SHOW_VARS()`；`vars` 仍未暴露在
  safe builtins 中。
- model schema、runtime 文档和 DeepSeek-TUI parity plan 已补充这些 helper 名称。

## 验证

- `/home/willamhou/.cargo/bin/cargo fmt --check`
- `/home/willamhou/.cargo/bin/cargo test rlm_python_repl_helpers`
- `/home/willamhou/.cargo/bin/cargo test rlm`
- `/home/willamhou/.cargo/bin/cargo test build_tool_specs_include_rlm`
- `git diff --check`
- `/home/willamhou/.cargo/bin/cargo test`
- `/home/willamhou/.cargo/bin/cargo package --allow-dirty`
