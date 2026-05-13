# DeepSeek-TUI ACP Tool Update Streaming Parity Spec

日期：2026-05-12

对比对象：`Hmbown/DeepSeek-TUI`，`main` HEAD `3382242`

## 背景

DeepSeekCode ACP adapter 已支持 `session/tools/list` / `session/tools/call`，
loaded runtime-thread sessions 还会把 tool call/result 写入 durable runtime items。
但 ACP 客户端当前只能看到最终 `session/tools/call` result，无法在调用过程中收到
tool started / completed 这类增量更新。DeepSeek-TUI 的 runtime/TUI 面更强调
streaming state；补 tool update notifications 可以继续收窄 ACP standard tool
streaming 差距。

## 目标

- `session/tools/call` 在最终 JSON-RPC result 前发送 `session/update` tool call
  running notification。
- 工具执行结束后发送 `session/update` tool result notification。
- loaded runtime-thread session 的 update 带上 runtime `turnId` / item id，方便客户端
  对齐 durable audit trail。
- 保持最终 `session/tools/call` result 形状不变。
- read-only new session 也发送 started/result updates，但没有 runtime ids。

## 非目标

- 不改变 ACP `session/prompt` streaming。
- 不实现多 chunk 工具 stdout/stderr 流。
- 不改变 MCP tool executor 行为。

## 验收标准

1. read-only ACP `session/tools/call` 返回顺序为 `tool_call_update`、
   `tool_result_update`、最终 JSON-RPC result。
2. loaded ACP `session/tools/call` 的 tool updates 包含 runtime `turnId` 和 item ids。
3. 最终 result 仍包含原本的 `content` / `isError` 结构和请求 id。

## 实现结果

- `session/tools/call` dispatch 现在返回三段响应：
  `session/update tool_call_update`、`session/update tool_result_update`、最终 JSON-RPC
  success result。
- read-only ACP session 的 tool updates 包含 tool name、status、result text 和
  `isError`。
- loaded runtime-thread ACP session 继续创建 durable assistant turn、
  `tool_call` item 和 `tool_result` item；tool updates 额外带出 `turnId`、
  `callItemId` 和 `resultItemId`。
- 最终 result 继续复用原 MCP tool result shape，并由 JSON-RPC dispatch 填回原请求 id。

## 验证

- `cargo fmt --check`
- `cargo test acp_session_tools`
- `cargo test acp_loaded_session_tools_call_write_file_uses_runtime_approval`
- `cargo test acp`
- `git diff --check`
- `cargo test`
- `cargo package --allow-dirty`
