# DeepSeek-TUI RLM Helper Alias Parity Spec

日期：2026-05-12

对比对象：`Hmbown/DeepSeek-TUI`，`main` HEAD `3382242`

## 背景

DeepSeekCode 已有 `rlm` 和 `rlm_batch`，通过 bounded child-agent analysis
模拟 DeepSeek-TUI 的 RLM 批量/递归分析能力。DeepSeek-TUI 文档和 README 中更常见
的入口名是 `rlm_query` 与 batched helper（`llm_query_batched` /
`rlm_query_batched`）。当前 DeepSeekCode 功能存在，但命名不利于对齐 DeepSeek-TUI
心智模型。

## 目标

- 暴露 `rlm_query` 作为 `rlm` 的兼容别名。
- 暴露 `rlm_query_batched` 作为 `rlm_batch` 的兼容别名。
- 别名遵守同样的 subagent depth gate，不绕过现有递归限制。
- Tool schema、registry names、error messages 和 docs 都能识别 alias 名称。

## 非目标

- 不实现完整 Python REPL sandbox。
- 不把 `rlm_batch` 从 bounded child-agent fan-out 改为 one-shot Flash API。
- 不提高当前并行 child-agent 上限。

## 验收标准

1. 默认 registry 在 subagent depth limit 以下包含 `rlm_query` 和
   `rlm_query_batched`。
2. 达到 subagent depth limit 时别名和原工具一起隐藏。
3. OpenAI/Anthropic tool schema builder 能输出两个 alias schema。
4. alias tool 的 missing-arg error 使用 alias tool name。
5. 单元测试覆盖 registry、schema、alias name/error 行为。

## 实现结果

- `RlmTool` 和 `RlmBatchTool` 支持显式 `tool_name`，同一执行路径可服务原名和
  DeepSeek-TUI-compatible alias。
- 默认 registry 在 subagent depth limit 以下同时注册 `rlm`、`rlm_query`、
  `rlm_batch`、`rlm_query_batched`。
- `rlm_query` / `rlm_query_batched` 已加入 OpenAI/Anthropic tool schema。
- alias 缺参错误会使用 alias 名称，避免模型看到 `rlm_query` 调用后收到
  `rlm requires ...` 这类不一致提示。

## 验证

- `cargo test rlm_alias_tools_report_alias_name_in_errors`
- `cargo test default_registry_includes_dispatch_subagent_only_below_max_depth`
- `cargo test build_tool_specs_include_rlm`
