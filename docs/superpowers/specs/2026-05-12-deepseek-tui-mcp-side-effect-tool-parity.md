# DeepSeek-TUI MCP Side-Effect Tool Parity Spec

日期：2026-05-12

对比对象：`Hmbown/DeepSeek-TUI`，`main` HEAD `3382242`

## 背景

DeepSeek-TUI 的 MCP 文档说明 self-hosted `serve --mcp` 会把 DeepSeek tools
暴露给其他 MCP clients，并示例说明 `shell` tool 会以
`mcp_deepseek_shell` 形式出现在 client 侧。同时它强调 MCP tools 会流经同一套
approval framework，side-effectful MCP tools 需要审批。

DeepSeekCode 本轮前的 `serve --mcp` 只暴露只读 workspace/runtime tools
以及 diagnostics/git 读取类工具。相比 DeepSeek-TUI，最大缺口是没有任何
受控 side-effect tool surface。

## 目标

先落地最小安全 slice：自托管 MCP server 可在显式 opt-in 环境变量下暴露
`run_shell`，用于 trusted MCP client 的测试/构建/只读 shell 类操作；默认仍不暴露，
直接调用也返回禁用错误。

## 非目标

- 本轮不暴露 `apply_patch`、文件写入或任意 shell。
- 本轮不实现跨 MCP client 的 durable approval prompt；没有显式 opt-in 时必须拒绝
  side-effect tool。
- 本轮不绕过 `run_shell` 现有 allowlist。

## 验收标准

1. 默认 `deepseek serve --mcp` 的 `tools/list` 不包含 `run_shell`。
2. 默认直接 `tools/call` `run_shell` 返回 MCP tool-level error，提示设置
   `DSCODE_MCP_ENABLE_SIDE_EFFECTS=1`。
3. 当 MCP server 环境变量 `DSCODE_MCP_ENABLE_SIDE_EFFECTS=1` 时，
   `tools/list` 包含 `run_shell`。
4. opt-in 后 `tools/call run_shell` 复用现有 `RunShellTool`，仍受安全命令
   allowlist 约束。
5. 文档说明该 tool 是显式 opt-in，不等同于完整 durable approval bridge。
6. 单元测试覆盖默认隐藏、默认拒绝和 opt-in 执行。

## 实施结果

已落地：

- `src/cli/commands/serve.rs`
  - `McpStdioState::allow_side_effect_tools`
  - `DSCODE_MCP_ENABLE_SIDE_EFFECTS=1` opt-in
  - `tools/list` 条件暴露 `run_shell`
  - `tools/call run_shell` 默认拒绝，opt-in 后复用 `RunShellTool`
  - 测试覆盖默认隐藏、默认拒绝和 opt-in 执行

验证：

- `/home/willamhou/.cargo/bin/cargo test mcp`

## 剩余差距

- 后续 durable approval bridge 已拆到
  `2026-05-12-deepseek-tui-mcp-durable-approval-parity.md`。
