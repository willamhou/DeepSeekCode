# DeepSeek-TUI RLM Batch Parity Spec

日期：2026-05-12

对比对象：`Hmbown/DeepSeek-TUI`，`main` HEAD `3382242`

## 背景

DeepSeek-TUI 的 RLM Python helpers 包含 `llm_query_batched` /
`rlm_query_batched`，用于把同一份上下文拆成多路分类、比较、抽取或 critique
子问题。DeepSeekCode 已有 `rlm` 单任务 child-agent wrapper，但缺少批量入口。

## 目标

新增 `rlm_batch` 工具：接受共享 `context` 和最多 4 个 `questions`，把每个问题渲染成
bounded RLM child task，并复用 `dispatch_subagents` 的并行执行、artifact 和 summary
路径。

## 非目标

- 不实现 Python REPL sandbox。
- 不提供任意数量批处理；沿用现有 parallel subagent 上限。
- 不绕过 subagent depth limit。

## 验收标准

1. Tool registry 在 subagent depth limit 内暴露 `rlm_batch`。
2. 达到 subagent depth limit 后隐藏 `rlm_batch`。
3. Model tool schema 包含 `rlm_batch` 的 `context`、`questions`、`strategy`、`steps`。
4. `questions` 支持 JSON string array，且拒绝空列表和超过 4 个问题。
5. `rlm_batch` 渲染 parallel child tasks，并复用 `dispatch_subagents` 执行。
6. 单元测试覆盖 batch task rendering、registry depth gate、model schema。

## 实施结果

已落地：

- `src/tools/rlm.rs`
  - `RlmBatchTool`
  - JSON string/object question parsing
  - 最大 4 个 batch questions
  - parallel child task rendering，复用 `DispatchSubagentsTool`
- `src/tools/registry.rs`
  - subagent depth limit 内注册 `rlm_batch`
  - depth limit 处隐藏 `rlm_batch`
- `src/model/deepseek.rs`
  - OpenAI/Anthropic tool schema 加入 `rlm_batch`

验证：

- `/home/willamhou/.cargo/bin/cargo test rlm`
- `/home/willamhou/.cargo/bin/cargo test default_registry_includes_dispatch_subagent_only_below_max_depth`
- `/home/willamhou/.cargo/bin/cargo test build_tool_specs_include_rlm`

## 剩余差距

- 仍未实现 DeepSeek-TUI 的 Python REPL sandbox；当前 batch parity 是
  bounded parallel child-agent wrapper。
