# DeepSeek-TUI ACP Permissioned Tool Bridge Parity Spec

日期：2026-05-12

对比对象：`Hmbown/DeepSeek-TUI`，`main` HEAD `3382242`

## 背景

DeepSeekCode 的 ACP adapter 已覆盖 session lifecycle、durable prompt 记录和
checkpoint replay/restore，但 ACP 客户端仍不能调用工具。DeepSeek-TUI 的核心
运行时能力依赖 permissioned tools；要接近 Claude/Codex editor-agent 体验，ACP
需要一个 scoped tool bridge，至少能让 loaded session 在 runtime thread 审批链路下
使用同一批 workspace tools。

## 目标

- ACP `initialize` 声明 session-scoped tools 能力。
- 新增 `session/tools/list`，返回当前 ACP session 可用工具。
- 新增 `session/tools/call`，调用当前 ACP session 工具。
- read-only tools 默认在 ACP session workspace 下执行。
- side-effect tools 只在 ACP session 绑定 runtime thread 时出现。
- side-effect tools 复用 runtime `permission_request` / `permission_response`
  审批链路，不新增绕过路径。

## 非目标

- 不实现完整 ACP 标准 tool-call streaming 协议。
- 不新增新的 tool implementation；先复用 MCP server 的工具定义和执行路径。
- 不允许未绑定 runtime thread 的 ACP session 调用写入或 shell side-effect tools。

## 验收标准

1. `initialize` response 声明 `sessionCapabilities.tools.readOnly=true` 和
   `permissioned=true`。
2. `session/tools/list` 对普通 `session/new` 返回 read-only tools，但不返回
   `run_shell` / `apply_patch` / `write_file`。
3. loaded runtime-thread session 的 `session/tools/list` 返回 permissioned tools。
4. `session/tools/call` 能在 session workspace 下调用 `read_file`。
5. loaded runtime-thread session 调用 `write_file` 时创建 permission request；
   approved 后写入文件。

## 实现结果

- `initialize` 的 `sessionCapabilities` 新增
  `tools.readOnly=true` 和 `tools.permissioned=true`。
- `serve --acp` 新增 `session/tools/list` 和 `session/tools/call`。
- ACP tool bridge 复用 MCP stdio server 的 tool definitions 和 executor。
- tool execution 会默认把 `cwd` 设为 ACP session workspace；`read_file`、
  `list_files`、`search_text` 的相对路径也会解析到该 workspace。
- 未绑定 runtime thread 的 ACP session 只展示 read-only tools。
- loaded runtime-thread session 展示 `run_shell`、`apply_patch`、`write_file`，
  并复用该 thread 的 durable approval events。

## 验证

- `cargo test acp_initialize_advertises_baseline_agent`
- `cargo test acp_session_tools_list_new_session_is_read_only`
- `cargo test acp_session_tools_call_reads_file_from_session_workspace`
- `cargo test acp_loaded_session_tools_call_write_file_uses_runtime_approval`
