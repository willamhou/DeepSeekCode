# DeepSeek-TUI MCP Write File Approval Parity Spec

日期：2026-05-12

对比对象：`Hmbown/DeepSeek-TUI`，`main` HEAD `3382242`

## 背景

DeepSeekCode MCP server 已经有 read-only workspace/runtime tools，并在 durable
approval 模式下暴露 `run_shell` 与 `apply_patch`。DeepSeek-TUI 的 MCP 文档明确把
side-effectful MCP tools 纳入 approval framework；文件写入是当前 MCP side-effect
surface 中除 patch 外最直接的缺口。

## 目标

- 新增 MCP `write_file` tool。
- `write_file` 只在 durable approval 模式下出现在 `tools/list`。
- 调用 `write_file` 必须走 runtime permission request / response。
- `write_file` 只允许写入 MCP workspace 下的相对路径。
- 自动创建父目录，但拒绝绝对路径、`..` 路径和 symlink escape。
- approval denied 时不写文件。

## 非目标

- 不在 trusted `DSCODE_MCP_ENABLE_SIDE_EFFECTS=1` 模式暴露 `write_file`。
- 不新增 delete/move 这类更高风险文件操作。
- 不改变 MCP config 或 dynamic MCP bridge 策略。

## 验收标准

1. 默认 MCP `tools/list` 不包含 `write_file`。
2. trusted side-effect 模式仍不包含 `write_file`。
3. durable approval 模式 `tools/list` 包含 `write_file`。
4. approval approved 后 `tools/call write_file` 写入 workspace 相对路径。
5. approval denied 后 `tools/call write_file` 不写文件。
6. unsafe path 被拒绝。

## 实现结果

- `serve --mcp` 新增 `write_file` tool dispatch。
- `tools/list` 默认和 trusted side-effect 模式都不展示 `write_file`。
- durable approval 模式展示 `write_file`，并通过
  `permission_request kind=write` / `permission_response` gate 后才写文件。
- 写入路径必须是相对 MCP workspace 的安全路径；绝对路径、`..`、空路径、symlink
  target 和 symlink parent escape 都会被拒绝。
- 写入会自动创建父目录，返回写入字节数和 create/overwrite 状态。

## 验证

- `cargo test mcp_tools_list_includes_write_tools_only_with_durable_approvals`
- `cargo test mcp_tools_call_executes_write_file_after_runtime_approval`
- `cargo test mcp_tools_call_rejects_write_file_after_runtime_denial`
- `cargo test mcp_tools_call_rejects_write_file_unsafe_path`
