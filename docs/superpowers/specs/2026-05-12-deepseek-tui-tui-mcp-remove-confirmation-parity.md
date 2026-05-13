# DeepSeek-TUI TUI MCP Remove Confirmation Parity Spec

日期：2026-05-12

对比对象：`Hmbown/DeepSeek-TUI`，`main` HEAD `3382242`

## 背景

DeepSeekCode TUI MCP manager 已经支持选中 server 后用 `e` / `d` / `x` /
`t` 执行 enable、disable、remove 和 tools detail。当前 `x` 会直接排队
`McpRemove`，对 MCP server config 这种破坏性动作缺少确认步骤。DeepSeek-TUI 的
manager 交互更接近完整管理面板；补确认弹窗可以降低误删风险，也收窄剩余
“confirmation modal” 差距。

## 目标

- MCP manager 中按 `x` 不直接删除，而是打开 remove confirmation modal。
- modal 显示待删除 server 名称和 config scope。
- `y` / `Enter` 确认后才排队 `TuiAction::McpRemove`。
- `n` / `Esc` 取消后不排队任何删除动作。
- 只影响 manager 选中 server 快捷键；command palette 的 `mcp remove <name>` 继续保持
  显式命令行为。

## 非目标

- 不新增鼠标确认按钮。
- 不改变 MCP config 文件结构或 remove handler。
- 不为 enable/disable 增加确认。

## 验收标准

1. manager 中按 `x` 后显示 remove confirmation modal，且 `drain_actions()` 为空。
2. modal 中按 `y` 或 `Enter` 后排队 `McpRemove { scope, name }`。
3. modal 中按 `n` 或 `Esc` 后不排队 remove，并清除 modal。
4. render 测试覆盖 modal 文案。

## 实现结果

- `TuiApp` 增加 MCP remove pending confirmation 状态。
- full-width MCP manager 中按 `x` 只打开 `MCP Remove Confirmation` modal，不立即排队
  `McpRemove`。
- modal 中 `y` / `Enter` 会排队原有 `TuiAction::McpRemove`，继续复用现有 config
  mutation handler。
- modal 中 `n` / `Esc` 会取消并清空 pending confirmation。
- manager key hint 更新为 `x remove (confirm)` / `x remove...`。

## 验证

- `cargo fmt --check`
- `cargo test mcp_manager`
- `cargo test mcp`
- `git diff --check`
- `cargo test`
- `cargo package --allow-dirty`
