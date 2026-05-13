# DeepSeek-TUI MCP Server Prompts Parity Spec

日期：2026-05-12

对比对象：`Hmbown/DeepSeek-TUI`，`main` HEAD `3382242`

## 背景

DeepSeekCode 已有 MCP client prompt discovery/get、REPL MCP prompt slash
commands、agent prompt bridge tools，以及 `serve --mcp` 的 read-only tools 和
resources。但 server 侧 `serve --mcp` 仍返回空 `prompts/list`，其它 MCP
clients 不能从 DeepSeekCode 发现常用 workflow prompts。

DeepSeek-TUI 的 MCP surface 会在 manager 和 model helper 中展示
tools/resources/prompts。要缩小 parity gap，DeepSeekCode 至少需要 server 侧
prompt serving baseline。

## 目标

让 `deepseek serve --mcp` 暴露只读内置 prompt templates，供其它 MCP client
发现和获取。

## 验收标准

1. `serve --mcp` 的 `prompts/list` 返回非空 prompt list。
2. `serve --mcp` 的 `prompts/get` 支持内置 prompt names：
   `review_code`、`explain_code`、`plan_task`。
3. `prompts/get` 校验必填参数，并返回 MCP message blocks。
4. 该 server prompt serving 不执行写入、不调用 shell、不触发审批。
5. 单元测试覆盖 prompt listing、prompt get 和缺参错误。
6. Runtime/install/release/parity docs 说明 server prompt surface。

## 非目标

- 这轮不从 `.dscode/commands` 动态导出用户自定义 slash commands。
- 这轮不开放 side-effectful MCP server tools。
- 这轮不实现完整 TUI MCP prompt pane。

## 实施结果

已落地：

- `src/cli/commands/serve.rs`
  - `prompts/list` 返回内置 workflow prompts
  - `prompts/get` 返回 `role=user` text content message blocks
  - 缺少 required prompt argument 时返回 JSON-RPC `-32602`
- `docs/runtime.md`
  - 记录 `serve --mcp` prompt capabilities 和 prompt list
- `docs/install.md`
  - `mcp add-self` 说明包含 server-side prompt templates
- `docs/release.md`
  - MCP stdio smoke 加入 `prompts/list` 和 `prompts/get`
- `docs/superpowers/plans/2026-05-10-deepseek-tui-parity.md`
  - Phase G2 更新 server-side prompt serving 状态
- `src/core/runtime.rs`
  - runtime record writes use temporary files plus rename, preventing concurrent
    readers from observing truncated JSON while approval/task events are being
    written

验证：

- `/home/willamhou/.cargo/bin/cargo fmt --check`
- `/home/willamhou/.cargo/bin/cargo test mcp`：77 passed
- `/home/willamhou/.cargo/bin/cargo test`：811 passed
- `git diff --check`
- `/home/willamhou/.cargo/bin/cargo package --allow-dirty`

## 剩余差距

- `serve --mcp` 还没有把用户自定义 `.dscode/commands/*.md` 映射成 MCP
  prompts。
- prompts 已进入基础 TUI 可滚动右侧 detail panel；完整 MCP manager screen
  仍未实现。
