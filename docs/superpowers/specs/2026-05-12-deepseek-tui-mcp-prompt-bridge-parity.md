# DeepSeek-TUI MCP Prompt Bridge Parity Spec

日期：2026-05-12

对比对象：`Hmbown/DeepSeek-TUI`，`main` HEAD `3382242`

## 背景

DeepSeek-TUI 的 MCP client 不只让用户手动执行 prompt discovery，也会把
MCP prompts 暴露给模型侧工作流。DeepSeekCode 已有
`deepseek mcp prompts`、`deepseek mcp prompt` 和 REPL slash prompt 调用，
但 agent bridge tools 只有 tools/resources/templates，缺 prompt discovery/get。

## 目标

补齐只读 MCP prompt bridge，让 agent 可以枚举 configured MCP servers 暴露的
prompts，并按需获取 prompt messages 作为上下文。

## 验收标准

1. MCP config 存在时，agent registry 暴露 `mcp_list_prompts` 和
   `mcp_get_prompt`。
2. `mcp_list_prompts` 复用 stdio / HTTP / SSE `prompts/list` summary。
3. `mcp_get_prompt` 复用 stdio / HTTP / SSE `prompts/get` summary，并接受可选
   JSON object arguments string。
4. 两个 bridge tools 都是只读发现/读取，不走 MCP side-effect approval gate。
5. OpenAI-compatible 和 Anthropic-compatible tool schemas 都包含 prompt bridge
   tools。
6. 单元测试覆盖 stdio prompt bridge execution、registry exposure/hiding 和
   model schema exposure。

## 非目标

- 基础 TUI 可滚动右侧 detail panel 已在后续 TUI MCP manager slice 覆盖 prompts；
  完整可滚动 MCP manager screen 仍非本轮目标。
- 这轮不把 prompt messages 自动转成下一轮用户输入；REPL slash prompt 已覆盖
  人工触发路径。

## 实施结果

已落地：

- `src/tools/mcp.rs`
  - `mcp_list_prompts`
  - `mcp_get_prompt`
- `src/tools/registry.rs`
  - bridge tools 在 project/user MCP config 存在时暴露
- `src/model/deepseek.rs`
  - OpenAI-compatible 和 Anthropic-compatible tool schemas

验证：

- `/home/willamhou/.cargo/bin/cargo fmt --check`
- `/home/willamhou/.cargo/bin/cargo test mcp`：74 passed
- `/home/willamhou/.cargo/bin/cargo test`：808 passed
- `git diff --check`
- `/home/willamhou/.cargo/bin/cargo package --allow-dirty`

## 剩余差距

- prompts 已进入基础 TUI 可滚动右侧 detail panel；完整 MCP manager screen
  仍未实现。
- agent 还不会自动把 MCP prompt result 提交为新的 user turn。
