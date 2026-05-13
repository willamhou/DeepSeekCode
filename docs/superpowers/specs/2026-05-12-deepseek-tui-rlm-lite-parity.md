# DeepSeek-TUI RLM-Lite Parity Spec

日期：2026-05-12

对比对象：`Hmbown/DeepSeek-TUI`，`main` HEAD `3382242`

## 背景

DeepSeek-TUI 把 `rlm` 定义为 Recursive Language Model 工具：长上下文或批量语义任务
可以进入 Python REPL，子模型通过 `llm_query` / `llm_query_batched` /
`rlm_query` helpers 做分块、比较、批量分类或递归 critique。

DeepSeekCode 已有 bounded `dispatch_subagent` / `dispatch_subagents`，但没有
名为 `rlm` 的一等工具；模型无法用 DeepSeek-TUI 提示词里的 RLM 习惯入口来处理
长 context synthesis。

## 目标

先落地 RLM-lite：提供 `rlm` 工具，把 `context` + `question` + optional
`strategy` 包装成 bounded child-agent analysis，复用现有 subagent depth limit、
hook、summary 和 policy 路径。

## 非目标

- 本轮不实现 Python REPL sandbox。
- 本轮不实现 `llm_query_batched` / `rlm_query_batched` helper API。
- 本轮不绕过现有 subagent depth limit。

## 验收标准

1. Tool registry 在 subagent depth limit 以内暴露 `rlm`。
2. 达到 subagent depth limit 后不再暴露 `rlm`，避免递归失控。
3. OpenAI/Anthropic tool schema 包含 `rlm` 的 `context`、`question`、
   `strategy` 和 `steps` 参数。
4. `rlm` 工具要求非空 `context` 和 `question`。
5. `rlm` 把输入渲染成 child-agent synthesis task，并复用
   `dispatch_subagent` 执行。
6. 单元测试覆盖 task rendering、registry exposure/depth gate 和 model schema。

## 实施结果

已落地：

- `src/tools/rlm.rs`
  - `RlmTool`
  - `render_rlm_task`
  - 复用 `DispatchSubagentTool`
- `src/tools/registry.rs`
  - subagent depth limit 内注册 `rlm`
  - depth limit 处隐藏 `rlm`
- `src/model/deepseek.rs`
  - OpenAI/Anthropic tool schema 加入 `rlm`
- `src/tools/mod.rs`
  - 注册 `rlm` module

验证：

- `/home/willamhou/.cargo/bin/cargo test rlm`
- `/home/willamhou/.cargo/bin/cargo test default_registry_includes_dispatch_subagent_only_below_max_depth`
- `/home/willamhou/.cargo/bin/cargo test build_tool_specs_include_rlm`

## 剩余差距

- Batched helper parity 已拆到
  `2026-05-12-deepseek-tui-rlm-batch-parity.md`。
- DeepSeek-TUI 的完整 RLM 仍是 Python REPL + helper runtime；DeepSeekCode
  当前是 bounded child-agent wrapper。
