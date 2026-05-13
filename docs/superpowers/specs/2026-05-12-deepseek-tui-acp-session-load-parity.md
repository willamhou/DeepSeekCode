# DeepSeek-TUI ACP Session Load Parity Spec

日期：2026-05-12

对比对象：`Hmbown/DeepSeek-TUI`，`main` HEAD `3382242`

## 背景

DeepSeekCode 的 ACP baseline 已覆盖 `initialize`、`session/new`、
`session/prompt`、`session/cancel` 和 `shutdown`，与 DeepSeek-TUI 当前
`docs/RUNTIME_API.md` 的初版 baseline 持平。下一档 editor/workbench 体验缺口是
ACP 无法发现或加载 DeepSeekCode 已有 durable runtime sessions。

## 目标

新增保守的 ACP session loading slice：

- `initialize` 宣告 `loadSession: true`
- `session/list` 返回 durable runtime sessions 摘要
- `session/load` 接受 runtime `sessionId` 和可选 `threadId`，把对应 session/thread
  的 workspace 绑定成进程内 ACP session

## 非目标

- 不把 ACP prompt 自动写回 durable turns/items。
- 不开放 ACP shell/file-write tools。
- 不实现 checkpoint replay。
- 不实现 permissioned tool bridge。

## 验收标准

1. `initialize` 返回 `loadSession: true`。
2. `session/list` 返回 runtime sessions，并支持 optional `limit`。
3. `session/load` 可加载 runtime session 的 active thread。
4. `session/load` 可显式加载同 session 下的 `threadId`。
5. 加载后的 ACP session 可作为 `session/prompt` 的 `sessionId` 使用。
6. 单元测试覆盖 initialize capability、session/list、session/load active thread 和
   thread/session mismatch rejection。

## 实施结果

已落地：

- `src/cli/commands/serve.rs`
  - `AcpStdioState` 持有 runtime store
  - `initialize` 宣告 `loadSession: true`
  - `session/list` 返回 durable runtime sessions
  - `session/load` 支持 active thread 和显式 `threadId`
  - 加载后的 ACP session 绑定 runtime session/thread workspace
  - thread/session mismatch 返回 JSON-RPC `-32602`

验证：

- `/home/willamhou/.cargo/bin/cargo test acp`

## 剩余差距

- ACP prompt 仍不自动写回 durable turns/items。
- ACP checkpoint replay 和 permissioned tool bridge 仍需单独设计。
