# DeepSeek-TUI ACP Tool Event Mapping Parity Spec

日期：2026-05-12

对比对象：`Hmbown/DeepSeek-TUI`，`main` HEAD `3382242`

## 背景

DeepSeekCode 已经通过 `session/tools/list` 和 `session/tools/call` 暴露 ACP
tool bridge，但 loaded runtime-thread session 的工具调用只同步返回结果，没有把
tool call / tool result 写回 runtime items。DeepSeek-TUI、Claude Code CLI 和 Codex
CLI 都强调可追踪的 tool timeline；ACP bridge 也需要把工具轨迹持久化到同一条
runtime thread。

## 目标

- loaded runtime-thread ACP `session/tools/call` 创建一个 assistant turn。
- 工具执行前写入 `tool_call` item。
- 工具执行后写入 `tool_result` item。
- 工具成功时 turn / items 标为 `completed`。
- 工具失败时 turn / items 标为 `failed`，但 JSON-RPC 仍按 MCP tool-result 形态返回
  `isError=true`。
- side-effect tool 的 `permission_request` 绑定到同一个 tool-call turn。

## 非目标

- 不实现完整 ACP tool streaming。
- 不改变非 loaded `session/new` 的 read-only tool call 行为。
- 不改变 MCP stdio server 的 event model。

## 验收标准

1. loaded ACP `session/tools/call write_file` 成功后 runtime thread 有一个
   assistant turn。
2. 同一 turn 下记录 `tool_call` 和 `tool_result` items。
3. `permission_request` event 的 `turn_id` 指向该 tool-call turn。
4. 返回给 ACP client 的 tool result 仍包含 `isError=false` 和文本结果。

## 实现结果

- `McpStdioState` 增加 optional `approval_turn_id`。
- ACP loaded-session `session/tools/call` 会先创建 `ACP tool call` assistant turn，
  再复用 MCP executor。
- `tool_call` item 在执行前写入 running，执行后更新为 completed/failed。
- `tool_result` item 记录工具输出，状态与工具结果一致。
- `run_shell` / `apply_patch` / `write_file` 的 durable approval request 会带上
  ACP tool-call turn id。

## 验证

- `cargo test acp_loaded_session_tools_call_write_file_uses_runtime_approval`
