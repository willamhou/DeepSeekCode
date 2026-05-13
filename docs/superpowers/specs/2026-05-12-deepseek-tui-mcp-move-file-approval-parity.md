# DeepSeek-TUI MCP Move File Approval Parity Spec

日期：2026-05-12

对比对象：`Hmbown/DeepSeek-TUI`，`main` HEAD `3382242`

## 背景

DeepSeekCode MCP server 已经补齐 `run_shell`、`apply_patch`、`write_file` 和
`delete_file` 这几类受审批保护的 side-effect tools。DeepSeek-TUI 的工作区操作更接近
完整文件管理 surface；当前 DeepSeekCode 仍缺少直接 rename/move 能力，必须通过读写删
组合绕路。为了继续收窄 MCP side-effect surface 差距，补 durable-approval-only
`move_file`。

## 目标

- 新增 MCP `move_file` tool。
- `move_file` 只在 durable approval 模式下出现在 `tools/list`。
- 调用 `move_file` 必须走 runtime permission request / response。
- 只允许移动 MCP workspace 下的安全相对路径。
- 拒绝绝对路径、`..` 路径、目录、symlink source 和已存在的 destination。
- approval denied 时不移动文件。

## 非目标

- 不在 trusted `DSCODE_MCP_ENABLE_SIDE_EFFECTS=1` 模式暴露 `move_file`。
- 不做递归目录移动。
- 不覆盖已存在的 destination。
- 不新增复制文件或批量移动。

## 验收标准

1. 默认 MCP `tools/list` 不包含 `move_file`。
2. trusted side-effect 模式仍不包含 `move_file`。
3. durable approval 模式 `tools/list` 包含 `move_file`。
4. approval approved 后 `tools/call move_file` 将 workspace 相对路径文件移动到目标路径。
5. approval denied 后 source 文件保留，destination 不创建。
6. unsafe source/destination path 被拒绝。

## 实现结果

- `serve --mcp` 新增 `move_file` tool dispatch。
- `tools/list` 默认和 trusted side-effect 模式都不展示 `move_file`。
- durable approval 模式展示 `move_file`，并通过
  `permission_request kind=write` / `permission_response` gate 后才移动文件。
- source 和 destination 必须是相对 MCP workspace 的安全路径；绝对路径、`..`、
  空路径、symlink parent escape、目录 source、symlink source 和已存在
  destination 都会被拒绝。
- 移动成功返回 source/destination 相对路径。

## 验证

- `cargo fmt --check`
- `cargo test move_file`
- `cargo test mcp_tools_list_includes_write_tools_only_with_durable_approvals`
- `cargo test mcp`
- `git diff --check`
- `cargo test`
- `cargo package --allow-dirty`
