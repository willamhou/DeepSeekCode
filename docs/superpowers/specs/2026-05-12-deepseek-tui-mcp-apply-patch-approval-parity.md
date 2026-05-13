# DeepSeek-TUI MCP Apply Patch Approval Parity Spec

日期：2026-05-12

对比对象：`Hmbown/DeepSeek-TUI`，`main` HEAD `3382242`

## 背景

DeepSeekCode MCP server 已有 read-only tools、trusted `run_shell` opt-in，以及
durable approval gated `run_shell`。DeepSeek-TUI 的 MCP server 目标更接近“把本地
tool surface 暴露给 MCP clients，但 side-effectful tools 进入统一 approval
framework”。当前 DeepSeekCode MCP server 仍缺少写文件 side-effect surface。

## 目标

新增 `apply_patch` MCP tool，但只在 durable approval 模式下暴露：

- 默认不列出、不执行
- `DSCODE_MCP_ENABLE_SIDE_EFFECTS=1` trusted direct mode 也不暴露 `apply_patch`
- 只有 `DSCODE_MCP_ENABLE_DURABLE_APPROVALS=1` 或
  `DSCODE_MCP_APPROVAL_THREAD_ID=<thread-id>` 时列出
- 调用时写入 `permission_request kind=write`，等待 matching
  `permission_response`
- approved 后复用现有 `ApplyPatchTool`

## 非目标

- 不提供无需审批的 MCP file-write。
- 不开放任意文件系统写入；继续复用 `ApplyPatchTool` 的 path scope 和 patch
  validation。
- 不改变现有 `run_shell` trusted direct mode。

## 验收标准

1. 默认 MCP `tools/list` 不包含 `apply_patch`。
2. trusted direct `DSCODE_MCP_ENABLE_SIDE_EFFECTS=1` 仍不包含 `apply_patch`。
3. durable approval 模式列出 `apply_patch`。
4. approved 后 `tools/call apply_patch` 可应用 unified patch。
5. denied 后返回 MCP tool-level error，且不应用 patch。
6. 单元测试覆盖 listed/approved/denied 路径。

## 实现结果

- `serve --mcp` 的 `tools/list` 仅在 durable approval thread 存在时暴露
  `apply_patch`。
- `tools/call apply_patch` 会默认使用 MCP workspace 作为 `cwd`，记录
  `permission_request kind=write`，等待 matching `permission_response` 后再调用
  现有 `ApplyPatchTool`。
- denial path 返回 MCP tool-level error，并且不会执行 patch。
- trusted direct `DSCODE_MCP_ENABLE_SIDE_EFFECTS=1` 仍只影响 `run_shell`，不会暴露
  MCP `apply_patch`。

## 验证

- `cargo test mcp_tools_list_includes_apply_patch_only_with_durable_approvals`
- `cargo test mcp_tools_call_executes_apply_patch_after_runtime_approval`
- `cargo test mcp_tools_call_rejects_apply_patch_after_runtime_denial`
- `cargo test mcp`
