# DeepSeek-TUI TUI MCP Manager Tabs Parity Spec

日期：2026-05-12

对比对象：`Hmbown/DeepSeek-TUI`，`main` HEAD `3382242`

## 背景

DeepSeekCode 已有 full-width MCP manager screen 和右侧 discovery detail panel，但
manager screen 仍主要是静态文本。DeepSeek-TUI 的 MCP 管理体验更接近交互式管理器：
用户能在不同 MCP surfaces 间切换，并快速聚焦 server/tool/prompt/resource 信息。

## 目标

给 DeepSeekCode TUI MCP manager 增加轻量 tab/filter 交互：

- manager screen 顶部显示 overview/tools/prompts/resources/templates/health tabs。
- 当前 detail kind 高亮为 active tab。
- command palette 支持 `mcp manager tab <tab>` 快速切换。
- command palette 支持 `mcp manager filter <query>` 对当前 manager detail 做行过滤。

## 非目标

- 不引入鼠标交互。
- 不实现远端 runtime 的 MCP config mutation。
- 不替换现有 command palette MCP CRUD。

## 验收标准

1. Manager screen 渲染 tab strip，并标识 active tab。
2. `mcp manager tab tools|prompts|resources|resource-templates|health|overview`
   触发对应 action 或 overview。
3. `mcp manager filter <query>` 设置 manager filter，`mcp manager filter` 清空。
4. Filter 只影响 full-width manager screen，不影响右侧 detail panel。
5. 单元测试覆盖 command parsing 和 filtered manager render。

## 实施结果

已落地：

- `src/tui.rs`
  - full-width MCP manager 渲染 overview/tools/prompts/resources/templates/health
    tab strip
  - active tab 以 `[tab]` 形式标识
  - `mcp manager tab <tab>` / `mcp open tab <tab>` 切换 manager detail
  - `mcp manager filter <query>` 设置行过滤，`mcp manager filter` 清空
  - filter 只应用于 full-width manager body

验证：

- `/home/willamhou/.cargo/bin/cargo test render_mcp_manager`
- `/home/willamhou/.cargo/bin/cargo test command_palette_requests_mcp_inventory_actions`

## 剩余差距

- 还没有键盘原生 tab cycling 或 per-server action menus；目前 tab/filter 通过
  command palette 驱动。
