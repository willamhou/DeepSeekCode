# DeepSeek-TUI TUI MCP Keyboard Parity Spec

日期：2026-05-12

对比对象：`Hmbown/DeepSeek-TUI`，`main` HEAD `3382242`

## 背景

DeepSeekCode TUI 已有 full-width MCP manager、manager tabs、filter、detail
discovery 和 `/mcp reload`。DeepSeek-TUI 文档强调 `/mcp` 是一个可直接操作的
manager，而不是只能通过命令字符串切换的静态报告。当前 DeepSeekCode manager
仍缺少键盘原生的 tab cycling / refresh 动作，交互效率低于 DeepSeek-TUI。

## 目标

- MCP manager 打开时，`Tab` 切到下一个 manager tab。
- MCP manager 打开时，`Shift+Tab` / `BackTab` 切到上一个 manager tab。
- MCP manager 打开时，`r` 触发 MCP reload/list refresh action。
- 快捷键只作用于 full-width MCP manager；普通 MCP detail panel 仍保持原有滚动和
  close 行为。

## 非目标

- 不新增可视化 per-server action menu。
- 不改变现有 command palette 语法。
- 不改变 `Tab` 在普通 TUI 主屏幕上的 Plan / Agent / YOLO mode cycling。

## 验收标准

1. full-width MCP manager 上 `Tab` 从 overview 请求 tools tab。
2. full-width MCP manager 上 `BackTab` 从 tools 请求 overview tab。
3. full-width MCP manager 上 `Tab` 从 health wrap 回 overview。
4. full-width MCP manager 上 `r` 触发 `TuiAction::McpList`。
5. 单元测试覆盖 keyboard tab cycling 和 reload action。

## 实现结果

- `TuiMcpDetailKind` 增加 next/previous tab order。
- `TuiApp::handle_key` 在 full-width MCP manager 打开时拦截 `Tab`、`BackTab`
  和 `r`；普通 MCP detail panel 和主屏幕 mode cycling 保持原行为。
- `Tab` / `BackTab` 会排队对应 `TuiAction::McpManager` 或
  `TuiAction::McpManagerDetails`，由现有 action handler 复用 discovery 路径。
- `r` 会排队 `TuiAction::McpList`，用于 refresh/reload MCP inventory。

## 验证

- `cargo test mcp_manager_keyboard_cycles_tabs_and_reloads`
- `cargo test render_mcp_manager`
- `cargo test command_palette_requests_mcp_inventory_actions`
