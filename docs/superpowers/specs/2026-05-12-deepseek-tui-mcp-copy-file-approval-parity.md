# DeepSeek-TUI MCP Copy File Approval Parity Spec

日期：2026-05-12

对比对象：`Hmbown/DeepSeek-TUI`，`main` HEAD `3382242`

## 背景

DeepSeekCode MCP server 已经有受 durable approval 保护的 `write_file`、
`delete_file` 和 `move_file`。DeepSeek-TUI 的工作区操作更接近完整文件管理面；
当前 DeepSeekCode 仍缺少直接复制文件能力，只能通过 read/write 组合绕路。补
`copy_file` 可以继续扩大 MCP side-effect surface，同时沿用现有 workspace path
安全边界和 runtime approval 审计链。

## 目标

- 新增 MCP `copy_file` tool。
- `copy_file` 只在 durable approval 模式下出现在 `tools/list`。
- 调用 `copy_file` 必须走 runtime permission request / response。
- 只允许复制 MCP workspace 下的安全相对路径文件。
- 拒绝绝对路径、`..` 路径、目录、symlink source 和已存在的 destination。
- approval denied 时不创建 destination，source 保留。

## 非目标

- 不在 trusted `DSCODE_MCP_ENABLE_SIDE_EFFECTS=1` 模式暴露 `copy_file`。
- 不做递归目录复制。
- 不覆盖已存在的 destination。
- 不复制权限、mtime 或扩展属性；只复制文件内容。

## 验收标准

1. 默认 MCP `tools/list` 不包含 `copy_file`。
2. trusted side-effect 模式仍不包含 `copy_file`。
3. durable approval 模式 `tools/list` 包含 `copy_file`。
4. approval approved 后 `tools/call copy_file` 复制 workspace 相对路径文件。
5. approval denied 后 source 文件保留，destination 不创建。
6. unsafe source/destination path 被拒绝。
7. 已存在 destination 被拒绝且不覆盖。

## 实现结果

- `serve --mcp` 新增 `copy_file` tool dispatch。
- `tools/list` 默认和 trusted side-effect 模式都不展示 `copy_file`。
- durable approval 模式展示 `copy_file`，并通过
  `permission_request kind=write` / `permission_response` gate 后才复制文件。
- source 和 destination 必须是相对 MCP workspace 的安全路径；绝对路径、`..`、
  空路径、symlink parent escape、目录 source、symlink source 和已存在
  destination 都会被拒绝。
- 复制成功返回 source/destination 相对路径。

## 验证

- `cargo fmt --check`
- `cargo test copy_file`
- `cargo test mcp_tools_list_includes_write_tools_only_with_durable_approvals`
- `cargo test mcp`
- `git diff --check`
- `cargo test`
- `cargo package --allow-dirty`
