# DeepSeek-TUI MCP Delete File Approval Parity Spec

日期：2026-05-12

对比对象：`Hmbown/DeepSeek-TUI`，`main` HEAD `3382242`

## 背景

DeepSeekCode MCP server 已经有 `run_shell`、`apply_patch`、`write_file` 三个
side-effect tools。DeepSeek-TUI 的文件操作 surface 覆盖写入和删除类操作；直接删除
文件仍需要走 patch 绕路。为了扩大 MCP side-effect surface，同时保持安全边界，
先补 durable-approval-only `delete_file`。

## 目标

- 新增 MCP `delete_file` tool。
- `delete_file` 只在 durable approval 模式下出现在 `tools/list`。
- 调用 `delete_file` 必须走 runtime permission request / response。
- 只允许删除 MCP workspace 下的安全相对路径。
- 拒绝绝对路径、`..` 路径、目录和 symlink target。
- approval denied 时不删除文件。

## 非目标

- 不在 trusted `DSCODE_MCP_ENABLE_SIDE_EFFECTS=1` 模式暴露 `delete_file`。
- 不做递归目录删除。
- 不新增 move/rename。

## 验收标准

1. 默认 MCP `tools/list` 不包含 `delete_file`。
2. trusted side-effect 模式仍不包含 `delete_file`。
3. durable approval 模式 `tools/list` 包含 `delete_file`。
4. approval approved 后 `tools/call delete_file` 删除 workspace 相对路径文件。
5. approval denied 后文件保留。
6. unsafe path 被拒绝。

## 实现结果

- `serve --mcp` 新增 `delete_file` tool dispatch。
- `tools/list` 默认和 trusted side-effect 模式都不展示 `delete_file`。
- durable approval 模式展示 `delete_file`，并通过
  `permission_request kind=write` / `permission_response` gate 后才删除文件。
- 删除路径必须是相对 MCP workspace 的安全路径；绝对路径、`..`、空路径、symlink
  parent escape、目录和 symlink target 都会被拒绝。
- 删除成功返回被删相对路径。

## 验证

- `cargo test mcp_tools_list_includes_write_tools_only_with_durable_approvals`
- `cargo test mcp_tools_call_executes_delete_file_after_runtime_approval`
- `cargo test mcp_tools_call_rejects_delete_file_after_runtime_denial`
- `cargo test mcp_tools_call_rejects_delete_file_unsafe_path`
