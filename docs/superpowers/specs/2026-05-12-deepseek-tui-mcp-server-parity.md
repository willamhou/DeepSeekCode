# DeepSeek-TUI MCP Server Parity Spec

日期：2026-05-12

对比对象：`Hmbown/DeepSeek-TUI`，`main` HEAD `3382242`

## 背景

DeepSeek-TUI 当前 README 和 `docs/MCP.md` 明确提供两类 MCP 能力：

- 作为 MCP client 连接 stdio / HTTP / SSE MCP servers
- 作为 MCP stdio server 暴露自身工具，入口为 `serve --mcp` 或 dispatcher
  `mcp-server`

DeepSeekCode 在本轮前已有 MCP client 能力，包括配置发现、tools/prompts
枚举、stdio / HTTP / SSE 调用、agent-side MCP bridge 和动态 MCP tool 注入。
缺口是 `deepseek serve --mcp` 仍直接返回 not implemented，导致外部 MCP
client 无法把 DeepSeekCode 当成本地工具服务器使用。

## 目标

落地第一版安全的 MCP stdio server mode，让 DeepSeekCode 至少能作为只读
MCP server 注册给其他本地 MCP clients。

## 非目标

本轮不做：

- 写文件、shell 执行、任务突变等 side-effectful MCP tools
- MCP prompt/resource serving
- `mcp add-self`
- ACP stdio adapter
- approval modal 与 MCP server tool call 的完整桥接

这些能力保留到后续 slice，避免第一版 MCP server 绕过现有审批模型。

## 验收标准

1. `deepseek serve --mcp` 不再报 not implemented。
2. stdio JSON-RPC 支持：
   - `initialize`
   - `notifications/initialized`
   - `tools/list`
   - `tools/call`
   - 空 `prompts/list` baseline；后续
     `2026-05-12-deepseek-tui-mcp-server-prompts-parity.md` 已补
     `prompts/list` / `prompts/get` 内置 prompt templates
   - 空 `resources/list`
3. `tools/list` 返回带 `inputSchema` 的工具定义。
4. `tools/call` 至少能调用：
   - workspace read tools：`list_files`, `read_file`, `search_text`
   - git/diagnostics read tools：`git_diff`, `git_log`, `git_show`,
     `git_blame`, `diagnostics`
   - runtime read tools：`runtime_health`, `runtime_list_sessions`,
     `runtime_list_threads`, `runtime_read_thread`, `runtime_list_tasks`,
     `runtime_read_task`
5. tool execution error 不让 MCP server 崩溃，而是作为 MCP tool result
   返回 `isError: true`。
6. 单元测试覆盖 initialize、tools/list、workspace tool call、runtime tool
   call。
7. release 文档包含 MCP stdio smoke 命令。

## 实施结果

已落地：

- `src/cli/commands/serve.rs`
  - `ServeAction::Mcp` 路由到 MCP stdio server
  - line-delimited JSON-RPC request loop
  - MCP response/error helpers
  - read-only workspace/git/diagnostics/runtime tool dispatch
  - read-only runtime resources via `resources/list` / `resources/read`
  - 4 个 focused 单元测试
- `docs/runtime.md`
  - 新增 MCP Stdio Server contract
- `docs/release.md`
  - 新增 `serve --mcp` release smoke
- `README.md`
  - 更新 feature surface
- `docs/superpowers/plans/2026-05-10-deepseek-tui-parity.md`
  - 刷新 DeepSeek-TUI 对比 HEAD
  - 新增 Phase G2: MCP Server Mode

## 验证

通过：

- `cargo fmt --check`
- `cargo test`：783 passed
- `cargo package --allow-dirty`
- `git diff --check`
- stdio smoke：

```bash
printf '%s\n' \
  '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"runtime_health","arguments":{}}}' \
  | target/debug/deepseek serve --mcp
```

返回 MCP JSON-RPC result，`isError:false`，tool text 内含
`"runtime":"mcp"`、`"service":"DeepSeekCode"`。

## 实施后差距复盘

本轮消除了 `serve --mcp` 完全缺失这一块，但相对 DeepSeek-TUI 仍有明显
MCP/server-mode residual gap：

- DeepSeek-TUI 支持 self-registration helper；DeepSeekCode 已在后续 slice
  落地 `mcp add-self`
- DeepSeek-TUI 的 server mode 更贴近完整 tool surface；DeepSeekCode 目前
  暴露只读工具和只读 runtime resources
- DeepSeek-TUI 文档区分 MCP server / HTTP runtime / ACP adapter；DeepSeekCode
  已补 MCP + HTTP，并已在后续 slice 落地最小 ACP stdio adapter
- DeepSeek-TUI TUI 内 `/mcp` manager 更成熟；DeepSeekCode 目前有 CLI MCP
  管理、agent bridge、TUI project manager commands 和基础可滚动右侧 discovery
  detail panel，但还没有完整 MCP manager screen

下一轮建议优先级：

1. MCP server side-effectful tools，必须先设计 durable approval bridge。
2. ACP session loading、checkpoint replay 和 permissioned tool bridging。
3. 完整 TUI MCP manager pane。
