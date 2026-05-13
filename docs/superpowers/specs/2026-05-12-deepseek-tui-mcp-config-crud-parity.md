# DeepSeek-TUI MCP Config CRUD Parity Spec

日期：2026-05-12

对比对象：`Hmbown/DeepSeek-TUI`，`main` HEAD `3382242`

## 背景

DeepSeek-TUI 的 MCP 文档提供常用配置管理命令：

- `mcp add <name> --command ... --arg ...`
- `mcp add <name> --url ...`
- `mcp remove <name>`
- `mcp enable <name>`
- `mcp disable <name>`
- `mcp validate`

DeepSeekCode 在本轮前已有 MCP discovery/call、prompt discovery/call、
`mcp init` 和 `mcp add-self`，但缺少通用 MCP server config CRUD。用户仍
需要手写 `.dscode/mcp.json` 或 `~/.config/dscode/mcp.json`。

## 目标

补齐常用 MCP config CRUD，让 DeepSeekCode 的 MCP 管理命令面接近
DeepSeek-TUI，同时继续保护用户已有 config，不默认覆盖同名 server。

## 验收标准

1. `deepseek mcp add <name> --command <cmd> [--arg <arg>]...` 写入 stdio
   server entry。
2. `deepseek mcp add <name> --url <url>` 写入 HTTP server entry；可通过
   `--transport sse` 明确旧式 SSE。
3. 支持 `--env KEY=VALUE`、`--header KEY=VALUE`、`--disabled`。
4. 默认写用户级 MCP config；`--project` 写当前项目级 config。
5. 已存在同名 server 时拒绝覆盖。
6. `mcp get <name>` 显示 merged inventory 中的 server detail。
7. `mcp remove|enable|disable <name>` 修改目标 scope 中的 server。
8. `mcp validate` 对 enabled servers 执行 tool discovery 硬验证，并汇总
   prompts/resources/resource-templates health。
9. 单元测试覆盖 CLI parsing、stdio/http add、duplicate 拒绝、
   enable/disable/remove mutation。

## 实施结果

已落地：

- `src/cli/app.rs`
  - `McpAction::{Add, Get, Remove, Enable, Disable, Validate}`
  - `mcp add` flag parsing
  - scoped mutation flags：`--user` / `--project`
- `src/cli/commands/mcp.rs`
  - generic MCP config JSON mutation helpers
  - add/remove/enable/disable implementation
  - merged-inventory `mcp get`
  - `mcp validate` through remote tool discovery plus
    prompts/resources/resource-templates health summary

## 剩余差距

- TUI 内已补最小 `/mcp` manager：默认 project-level mutation、显式
  `mcp user ...` mutation，以及 tools/prompts/resources/templates 可滚动右侧
  明细 panel；完整独立 MCP manager screen 仍未实现。
- `mcp validate` 已有 tools/prompts/resources/templates health summary；还没有
  DeepSeek-TUI 那种独立 full-screen health view。
- MCP server side-effectful tools 仍是后续工作；prompt/resource serving 已有
  read-only baseline。
