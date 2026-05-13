# DeepSeek-TUI TUI MCP Server Action Menu Parity Spec

日期：2026-05-12

对比对象：`Hmbown/DeepSeek-TUI`，`main` HEAD `3382242`

## 背景

DeepSeekCode TUI 已有 full-width MCP manager、tab cycling、filter、reload，以及
command-palette 层面的 `mcp enable|disable|remove <name>`。DeepSeek-TUI 文档强调
`/mcp` manager 支持 init/add/enable/disable/remove/validate/reload 等窄 manager
actions。当前 DeepSeekCode 仍需要用户手动输入 server 名称，manager 视图本身没有
选中 server 后直接执行动作的菜单。

## 目标

- full-width MCP manager 从当前 manager summary 解析 server entries。
- manager 视图显示选中 server 和可用动作提示。
- `n` / `p` 在 server entries 间循环选择。
- `e` / `d` / `x` 分别对选中 server 请求 enable / disable / remove。
- `t` 对选中 server 打开 manager-scoped tools detail。
- 仅对 `source=project|user` 的 server 发出可写 config action；其它 source 给出
  明确状态提示。

## 非目标

- 不实现鼠标菜单。
- 不新增确认弹窗；沿用现有 command-palette MCP mutation 行为。
- 不改变 MCP config 文件结构或 merge 规则。

## 验收标准

1. MCP manager 渲染时显示选中 server action strip。
2. `n` / `p` 可以切换选中 server 并更新 status。
3. `e` / `d` / `x` 会排队对应 `TuiAction`，scope/name 来自 server summary。
4. `t` 会排队 `TuiAction::McpManagerDetails { kind: Tools, server: Some(...) }`。
5. 单元测试覆盖 render、selection、enable/disable/remove/tools action。

## 实现结果

- `TuiApp` 增加 selected-server index，并从 full-width manager summary 的
  `MCP servers:` 行解析 `name`、`source`、`enabled`。
- MCP manager body 渲染 `Server actions:` strip，显示当前选中 server、source、
  enabled state，以及 `n/p/e/d/x/t/r` 快捷键。
- manager 打开时，`n` / `p` 循环选择 server；`e` / `d` / `x` 会排队现有
  `McpSetEnabled` / `McpRemove` actions；`t` 会排队 manager-scoped tools detail。
- source 只接受 `project` / `user`；其它 source 不会触发写 config action。

## 验证

- `cargo test mcp_manager_server_action_strip_renders_selection`
- `cargo test mcp_manager_keyboard_actions_target_selected_server`
- `cargo test mcp_manager_keyboard_cycles_tabs_and_reloads`
