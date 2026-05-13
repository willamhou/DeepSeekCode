# DeepSeek-TUI MCP Durable Approval Parity Spec

日期：2026-05-12

对比对象：`Hmbown/DeepSeek-TUI`，`main` HEAD `3382242`

## 背景

DeepSeek-TUI 的 MCP 文档说明 side-effectful MCP tools 会经过同一套 approval
framework。DeepSeekCode 已有 MCP server `run_shell` 的显式 trusted opt-in，但
`DSCODE_MCP_ENABLE_SIDE_EFFECTS=1` 是直通执行，不会把审批请求写入 durable runtime，
因此 TUI approval modal 无法接管外部 MCP client 发起的 side-effect tool call。

## 目标

增加第二种更安全的 side-effect MCP 模式：当 MCP server 启用 durable approvals 时，
`run_shell` 会出现在 `tools/list` 中；`tools/call run_shell` 先把
`permission_request` 写入 runtime thread，等待 TUI 或 HTTP runtime 写回
`permission_response`，批准后才执行，拒绝后返回 MCP tool-level error。

## 非目标

- 不默认暴露 side-effect MCP tools。
- 不移除已有 `DSCODE_MCP_ENABLE_SIDE_EFFECTS=1` trusted 直通模式。
- 不暴露 `apply_patch` 或文件写入 MCP tools。
- 不改变 `run_shell` 的 safe-command 限制。

## 验收标准

1. 默认 MCP server 仍不列出 `run_shell`，直接调用仍返回禁用错误。
2. durable approval 模式会列出 `run_shell`，但调用前必须写入
   `permission_request`。
3. 收到 matching `permission_response=approved` 后才执行 `run_shell`。
4. 收到 matching `permission_response=denied` 后返回 MCP tool-level error。
5. approval request 复用 runtime store，TUI 现有 approval modal 能读取。
6. 单元测试覆盖 listed/approved/denied 路径。

## 实施结果

已落地：

- `src/cli/commands/serve.rs`
  - `DSCODE_MCP_ENABLE_DURABLE_APPROVALS=1` 自动创建 MCP approval runtime
    thread
  - `DSCODE_MCP_APPROVAL_THREAD_ID=<thread-id>` 可复用已有 runtime thread
  - durable approval 模式下 `tools/list` 暴露 `run_shell`
  - `tools/call run_shell` 写入 `permission_request`，等待 matching
    `permission_response`
  - approved 后执行 `RunShellTool`，denied 后返回 MCP tool-level error
  - trusted `DSCODE_MCP_ENABLE_SIDE_EFFECTS=1` 直通路径保留

验证：

- `/home/willamhou/.cargo/bin/cargo test mcp`

## 剩余差距

- 当前 durable bridge 只覆盖 `run_shell`；DeepSeek-TUI 的完整 side-effect
  surface 还包括更广泛的 tool families，但继续扩大前需要逐项绑定审批和写入策略。
