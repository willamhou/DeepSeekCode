# DeepSeek-TUI MCP Resources Parity Spec

日期：2026-05-12

对比对象：`Hmbown/DeepSeek-TUI`，`main` HEAD `3382242`

## 背景

DeepSeek-TUI 的 MCP server mode 支持 `resources/list`，会暴露 workspace root
以及历史 session 资源。DeepSeekCode 在上一轮已实现 MCP stdio server tools，
但 `resources/list` 仍返回空数组，外部 MCP clients 只能通过 tools/call 主动
查询 runtime 信息，不能通过标准 resource surface 发现上下文。

## 目标

补齐 read-only MCP resource serving，让 DeepSeekCode 能把 workspace 和 durable
runtime records 作为 MCP resources 暴露给其他本地 clients。

## 非目标

本轮不做：

- resource mutation
- arbitrary file reading through `resources/read`
- MCP prompt serving
- side-effectful MCP tools

## 验收标准

1. `resources/list` 返回 workspace root resource。
2. `resources/list` 返回 runtime sessions、threads、tasks resources。
3. `resources/read` 支持：
   - `file://<workspace>` workspace metadata
   - `deepseekcode://runtime/sessions/<id>`
   - `deepseekcode://runtime/threads/<id>`，包含 turns/items
   - `deepseekcode://runtime/tasks/<id>`
4. 未知 URI 返回 JSON-RPC error，不让 server 崩溃。
5. 单元测试覆盖 resource listing 和读取 runtime thread JSON。
6. release smoke 覆盖 `resources/list`。

## 实施结果

已落地：

- `src/cli/commands/serve.rs`
  - `McpStdioState` 记录 workspace
  - `resources/list` 返回 workspace/session/thread/task resources
  - `resources/read` 返回 MCP `contents` array
  - focused tests 覆盖 listing 和 runtime thread read
- `docs/runtime.md`
  - 新增 MCP resource URI contract
- `docs/release.md`
  - MCP smoke 加入 `resources/list`
- `docs/superpowers/plans/2026-05-10-deepseek-tui-parity.md`
  - Phase G2 标记 read-only resources landed

## 剩余差距

- MCP prompt serving 已在后续
  `2026-05-12-deepseek-tui-mcp-server-prompts-parity.md` 切片补齐
  read-only baseline。
- MCP server 仍未开放 side-effectful tools；这需要 durable approval bridge。
- TUI `/mcp` manager 已有最小 project-level command palette 和基础可滚动右侧
  tools/prompts/resources/templates detail panel；完整 manager screen
  仍未实现。
