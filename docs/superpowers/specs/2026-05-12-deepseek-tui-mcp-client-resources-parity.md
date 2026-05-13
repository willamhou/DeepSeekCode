# DeepSeek-TUI MCP Client Resources Parity Spec

日期：2026-05-12

对比对象：`Hmbown/DeepSeek-TUI`，`main` HEAD `3382242`

## 背景

DeepSeek-TUI 的 MCP client 不只发现和调用 tools，也会发现 resources、
resource templates 和 prompts，并把 `list_mcp_resources` / `mcp_read_resource`
这类只读能力暴露给模型。

DeepSeekCode 在本轮前已有 MCP tools、prompts、config CRUD、`serve --mcp`
resources server 端，但 client 侧缺少对远端 MCP resources 的
`resources/list` / `resources/read` 支持。

## 目标

补齐 DeepSeekCode 的 MCP client resource discovery/read surface，让用户和
agent 都能读取 configured MCP servers 暴露的只读 resources。

## 验收标准

1. `deepseek mcp resources [server]` 对 stdio / HTTP / SSE MCP server 执行
   `resources/list`，展示 resource name、URI、description 和 mime type。
2. `deepseek mcp resource <server> <uri>` 对 stdio / HTTP / SSE MCP server
   执行 `resources/read`，展示 text/blob resource contents 和 mime type。
3. CLI parsing 拒绝错误参数形状。
4. Agent registry 在 MCP config 存在时暴露只读 bridge tools：
   `mcp_list_resources` 和 `mcp_read_resource`。
5. 这两个 agent bridge tools 不走 MCP side-effect approval gate。
6. 单元测试覆盖 request builders、result parsers、HTTP transport summary、
   stdio bridge tool execution、registry exposure。

## 非目标

- `resources/templates/list` 已在后续
  `2026-05-12-deepseek-tui-mcp-resource-templates-parity.md` 切片实现。
- 基础 TUI 可滚动右侧 detail panel 已在后续 TUI MCP manager slice 覆盖 resources；
  完整可滚动 MCP manager screen 仍非本轮目标。
- 这轮不改变 side-effectful MCP tool approval policy。

## 实施结果

已落地：

- `src/cli/app.rs`
  - `McpAction::Resources`
  - `McpAction::Resource`
  - CLI parsing for `mcp resources [server]` and
    `mcp resource <server> <uri>`
- `src/cli/commands/mcp.rs`
  - stdio / HTTP / SSE `resources/list`
  - stdio / HTTP / SSE `resources/read`
  - resource request builders and result parsers
  - public summaries for agent bridge tools
- `src/tools/mcp.rs`
  - `mcp_list_resources`
  - `mcp_read_resource`
- `src/tools/registry.rs`
  - bridge tools exposed when project/user MCP config exists
- `src/model/deepseek.rs`
  - OpenAI-compatible and Anthropic-compatible tool schemas for resource tools

验证：

- `/home/willamhou/.cargo/bin/cargo test mcp`

## 剩余差距

- resources 已进入基础 TUI 可滚动右侧 detail panel；完整 MCP manager screen
  仍未实现。
