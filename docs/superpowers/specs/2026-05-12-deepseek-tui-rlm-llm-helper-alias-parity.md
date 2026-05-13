# DeepSeek-TUI RLM LLM Helper Alias Parity Spec

日期：2026-05-12

对比对象：`Hmbown/DeepSeek-TUI`，`main` HEAD `3382242`

## 背景

DeepSeek-TUI 的 RLM Python REPL 暴露 `llm_query`、`llm_query_batched`、
`rlm_query`、`rlm_query_batched` helper names，并且 batched helper 支持 1-16
个独立请求。DeepSeekCode 已有 `rlm`、`rlm_batch`、`rlm_query`、
`rlm_query_batched`，但缺少 `llm_*` 兼容名，且 batch 上限仍是 4。

## 目标

- 新增 `llm_query`，作为 `rlm` 的兼容别名。
- 新增 `llm_query_batched`，作为 `rlm_batch` 的兼容别名。
- 将 RLM batch questions 上限从 4 提升到 16。
- 更新 OpenAI/Anthropic static tool schema。
- 更新 registry depth gating 测试与文档。

## 非目标

- 不在本切片实现完整 Python REPL runtime。
- 不改变 `rlm` / `rlm_batch` 的 child-agent execution strategy。
- 不改变 subagent depth limit。

## 验收标准

1. 默认 depth 下 registry 包含 `llm_query` 和 `llm_query_batched`。
2. 达到 max subagent depth 时不暴露 `llm_*` RLM aliases。
3. model tool schema 包含 `llm_query` 和 `llm_query_batched`。
4. `rlm_batch` 接受最多 16 个 questions，并拒绝第 17 个。

## 实现结果

- `RlmTool` registry 现在注册 `llm_query`，与 `rlm` / `rlm_query` 复用同一执行路径。
- `RlmBatchTool` registry 现在注册 `llm_query_batched`，与 `rlm_batch` /
  `rlm_query_batched` 复用同一执行路径。
- `MAX_RLM_BATCH_QUESTIONS` 从 4 提升到 16。
- OpenAI / Anthropic static tool schema 增加 `llm_query` 和
  `llm_query_batched`，并把 batched schema 描述更新为 16。
- registry depth-gating 测试覆盖新增 aliases。

## 验证

- `cargo test rlm`
- `cargo test build_tool_specs_include_rlm`
- `cargo test default_registry_includes_dispatch_subagent_only_below_max_depth`
