# DeepSeek-TUI ACP Durable Prompt Parity Spec

日期：2026-05-12

对比对象：`Hmbown/DeepSeek-TUI`，`main` HEAD `3382242`

## 背景

ACP `session/list` / `session/load` 已经能把 durable runtime sessions 映射进
ACP 进程内 session，但加载后的 `session/prompt` 仍只返回临时响应，不会把 editor
里的交互写回 DeepSeekCode runtime。相比完整 workbench/editor 体验，这会让 TUI、
HTTP runtime 和 ACP 之间的会话状态断开。

## 目标

当 ACP session 由 `session/load` 绑定到 runtime thread 时，`session/prompt` 在
模型返回后记录 durable runtime turns/items：

- user turn + user message item
- assistant turn + assistant message item
- 若模型返回 token usage，则记录 usage source `acp`

## 非目标

- 不把全新的 `session/new` 自动创建 durable runtime thread。
- 不开放 ACP shell/file-write tools。
- 不实现 checkpoint replay。
- 不实现 permissioned tool bridge。

## 验收标准

1. `session/new` 的临时 ACP session 行为不变。
2. `session/load` 后的 `session/prompt` 仍返回 `session/update` 和
   `stopReason: "end_turn"`。
3. loaded runtime thread 中新增 user/assistant 两个 turns。
4. loaded runtime thread 中新增 user/assistant 两个 message items。
5. token usage 存在时以 source `acp` 写入 runtime usage。
6. 单元测试覆盖 loaded prompt 的 durable turns/items 记录。

## 实施结果

已落地：

- `src/cli/commands/serve.rs`
  - `acp_run_prompt` 返回 model usage
  - loaded runtime-thread ACP sessions 在 `session/prompt` 后记录 durable user
    turn/item 和 assistant turn/item
  - usage 存在时写入 runtime usage，source 为 `acp`
  - `session/new` 临时 session 不写 runtime thread

验证：

- `/home/willamhou/.cargo/bin/cargo test acp`

## 剩余差距

- ACP checkpoint replay 和 permissioned tool bridge 仍需单独设计。
