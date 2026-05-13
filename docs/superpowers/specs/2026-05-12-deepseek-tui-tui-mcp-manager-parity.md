# DeepSeek-TUI TUI MCP Manager Parity Spec

日期：2026-05-12

对比对象：`Hmbown/DeepSeek-TUI`，`main` HEAD `3382242`

## 背景

DeepSeek-TUI 在 TUI slash command 中提供 `/mcp` 管理入口，常用操作包括：

- `/mcp` / `/mcp list` / `/mcp status`
- `/mcp init`
- `/mcp add stdio <name> <command> [args...]`
- `/mcp add http <name> <url>`
- `/mcp add sse <name> <url>`
- `/mcp enable|disable|remove <name>`
- `/mcp tools|prompts|resources|resource-templates [server]`
- `/mcp validate`
- `/mcp reload`

DeepSeekCode 在本轮前已有 CLI 级 MCP config CRUD、MCP client discovery/call、
`serve --mcp` 和 `mcp add-self`，但 TUI command palette 仍没有 MCP 管理入口。

## 目标

补齐 TUI 内最小可用的 MCP manager，使用户不离开 `deepseek tui` 就能查看、
初始化和维护项目级 MCP server 配置，触发基础验证，并查看 configured MCP
servers 暴露的 tools/prompts/resources/templates 明细。

## 验收标准

1. `:mcp`、`:mcp list`、`:mcp status`、`:mcp reload` 读取当前 MCP inventory
   并把 server 数、enabled 数和 server names 摘要显示到状态栏。
2. `:mcp init` 创建项目级 `.dscode/mcp.json`；`:mcp init --force` 可覆盖。
3. `:mcp add stdio <name> <command> [args...]` 写入项目级 stdio server。
4. `:mcp add http <name> <url>` 和 `:mcp add sse <name> <url>` 写入项目级
   remote server。
5. `:mcp enable <name>`、`:mcp disable <name>`、`:mcp remove <name>` 修改项目级
   config。
6. `:mcp user add|enable|disable|remove ...` 修改用户级 config；未带 `user`
   scope 的 mutation 命令保持项目级默认。
7. `:mcp validate` 复用现有 MCP health summary 验证 enabled servers，并把
   tools/prompts/resources/resource-templates health 摘要显示到可滚动右侧 panel
   和状态栏。
8. `:mcp` / `:mcp manager` 打开独立 MCP manager body screen，展示 merged
   inventory、配置来源、server 摘要和常用操作入口；`Esc` 或 `mcp close`
   关闭并回到主 workbench。
9. `:mcp manager tools|prompts|resources|resource-templates [server]` 复用现有
   CLI MCP discovery summary，并把明细渲染到 full-width manager body screen。
10. `:mcp tools [server]`、`:mcp prompts [server]`、`:mcp resources [server]`
   和 `:mcp resource-templates [server]` 保留右侧 TUI panel 明细视图。
11. HTTP runtime TUI mode 对 MCP manager command 给出明确边界提示，而不是误以为
   已修改远端 config。
12. 单元测试覆盖 command palette action parsing、本地 handler 对 project/user
    MCP config 的 add/disable/enable/remove/list/init 行为，以及右侧 MCP
    detail panel 和独立 manager screen 渲染/滚动。

## 非目标

- 这轮不实现 user-level `mcp init`；user config mutation 由 add/enable/disable/remove
  创建或修改目标 config。

## 实施结果

已落地：

- `src/tui.rs`
  - `TuiAction::{McpList, McpInit, McpAddStdio, McpAddRemote, McpRemove,
    McpSetEnabled, McpDetails, McpManagerDetails, McpValidate, McpManager}`
  - command palette 解析 `mcp list/status/reload/tools/prompts/resources/resource-templates/init/add/enable/disable/remove/validate`
  - command palette 解析 `mcp user add|enable|disable|remove ...`，显式选择
    user-level MCP config scope
  - `mcp` / `mcp manager` 打开独立 MCP manager body screen
  - `mcp manager tools|prompts|resources|resource-templates [server]` 在
    manager body screen 中展示 discovery 明细
  - 右侧 panel 在 MCP detail 存在时渲染并滚动
    tools/prompts/resources/templates 明细
- `src/cli/commands/tui.rs`
  - 本地 file-backed TUI handler 写入项目级 `.dscode/mcp.json`
  - 本地 file-backed TUI handler 可写用户级 MCP config
  - 本地 file-backed TUI handler 生成独立 MCP manager summary
  - 本地 file-backed TUI handler 复用 CLI MCP discovery summary 生成 detail
    panel 内容
  - `mcp validate` 也把 health summary 渲染到右侧 detail panel
  - HTTP runtime TUI handler 对 MCP manager command 返回本地边界提示
- `src/cli/commands/mcp.rs`
  - 复用 CLI MCP mutation helper
  - 增加 `mcp_status_summary` 和 `validate_servers_summary` 供 TUI 状态栏使用
  - 复用 `list_remote_*_summary` 供 TUI MCP detail panel 使用
  - `validate_servers_summary` 汇总 tools/prompts/resources/resource-templates
    health
- `docs/tui.md`、`docs/runtime.md`、`README.md`、`docs/install.md`、
  `docs/release.md` 同步公开能力说明

验证：

- `/home/willamhou/.cargo/bin/cargo test mcp`

## 剩余差距

- Full-width manager screen 和右侧 detail panel 现在都能显示
  tools/prompts/resources/templates summary；tab/filter 选择已拆到
  `2026-05-12-deepseek-tui-tui-mcp-manager-tabs-parity.md`。
