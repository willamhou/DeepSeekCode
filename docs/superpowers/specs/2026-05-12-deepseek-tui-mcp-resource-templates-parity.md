# DeepSeek-TUI MCP Resource Templates Parity Spec

日期：2026-05-12

对比对象：`Hmbown/DeepSeek-TUI`，`main` HEAD `3382242`

## 背景

DeepSeek-TUI 的 MCP client exposes both resource listing and resource template
listing to the model (`list_mcp_resource_templates`). DeepSeekCode 已在上一片
补齐 `resources/list` 和 `resources/read`，但还缺 `resources/templates/list`。

## 目标

补齐 MCP resource templates discovery，让 CLI 和 agent 都能查看 configured
MCP servers 暴露的 resource templates。

## 验收标准

1. `deepseek mcp resource-templates [server]` 对 stdio / HTTP / SSE server
   执行 `resources/templates/list`。
2. 输出 template name、URI template、description 和 mime type。
3. Agent registry 在 MCP config 存在时暴露只读 `mcp_list_resource_templates`。
4. 该 bridge tool 不走 MCP side-effect approval gate。
5. 单元测试覆盖 request builder、result parser、HTTP transport summary、
   stdio bridge tool execution、registry/model schema exposure。

## 非目标

- 这轮不实现根据 template 自动展开 URI。
- 基础 TUI 可滚动右侧 detail panel 已在后续 TUI MCP manager slice 覆盖 templates；
  完整可滚动 MCP manager screen 仍非本轮目标。

## 实施结果

已落地：

- `src/cli/app.rs`
  - `McpAction::ResourceTemplates`
  - CLI parsing for `mcp resource-templates [server]`
  - `mcp templates [server]` alias
- `src/cli/commands/mcp.rs`
  - stdio / HTTP / SSE `resources/templates/list`
  - resource template request builder and result parser
  - public summary helper for agent bridge tool execution
- `src/cli/commands/serve.rs`
  - `deepseek serve --mcp` responds to `resources/templates/list` with runtime
    session/thread/task URI templates, so self-registration remains compatible
    with the client command and no longer returns an empty template list
- `src/tools/mcp.rs`
  - `mcp_list_resource_templates`
- `src/tools/registry.rs`
  - bridge tool exposed when project/user MCP config exists
- `src/model/deepseek.rs`
  - OpenAI-compatible and Anthropic-compatible tool schema for template listing

验证：

- `/home/willamhou/.cargo/bin/cargo fmt --check`
- `/home/willamhou/.cargo/bin/cargo test mcp`：77 passed
- `/home/willamhou/.cargo/bin/cargo test`：811 passed
- `git diff --check`
- `/home/willamhou/.cargo/bin/cargo package --allow-dirty`

## 剩余差距

- templates 已进入基础 TUI 可滚动右侧 detail panel；完整 MCP manager screen
  仍未实现。
- 还没有从 template 自动展开 URI 并联动 `mcp_read_resource`。
