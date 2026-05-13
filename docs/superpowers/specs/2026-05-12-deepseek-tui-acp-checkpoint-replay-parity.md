# DeepSeek-TUI ACP Checkpoint Replay Parity Spec

日期：2026-05-12

对比对象：`Hmbown/DeepSeek-TUI`，`main` HEAD `3382242`

## 背景

DeepSeekCode 已有 rollback snapshots，并且 `deepseek exec` / TUI agent runs
会把 snapshot 绑定到 runtime turn id。ACP adapter 目前只支持 session
new/list/load/prompt/cancel，外部编辑器客户端无法通过 ACP 发现或读取这些
checkpoint。DeepSeek-TUI 文档把 ACP 标为 conservative baseline，但我们的目标是
向 Codex/Claude Code 风格的 editor-agent 恢复能力靠拢，因此先补只读 checkpoint
replay surface。

## 目标

- ACP `initialize` 明确声明只读 checkpoint replay 能力。
- 新增 `session/checkpoints`，列出 rollback snapshots。
- `session/checkpoints` 带 `sessionId` 时按 loaded ACP session 的 runtime thread
  过滤；未绑定 runtime thread 的 session 返回空列表。
- 新增 `session/checkpoint/read`，读取 checkpoint manifest。
- `session/checkpoint/read` 支持 `includePatch=true` 返回 unified diff patch。
- 支持通过 snapshot id 或绑定的 runtime turn id 读取 checkpoint。

## 非目标

- 不通过 ACP 应用 restore；写 worktree 仍走本地 `deepseek restore ... --apply`。
- 不新增 ACP file-write tools。
- 不改变 rollback snapshot 存储格式。

## 验收标准

1. `initialize` response 包含 `sessionCapabilities.checkpoints.readOnly=true`。
2. `session/checkpoints` 返回 checkpoint list。
3. loaded ACP session 的 `session/checkpoints` 只返回绑定到该 runtime thread 的
   checkpoints。
4. `session/checkpoint/read includePatch=true` 返回 checkpoint manifest 和 patch。
5. 单元测试覆盖 list/read/includePatch/turn-id lookup。

## 实现结果

- `serve --acp` state 现在持有同一 config dir 下的 `RollbackStore`。
- `initialize` 的 `sessionCapabilities` 增加
  `checkpoints.readOnly = true`。
- `session/checkpoints` 返回 rollback snapshot manifests；传入 loaded ACP
  `sessionId` 时按该 session 的 runtime thread id 过滤，未绑定 runtime thread
  的 session 返回空列表。
- `session/checkpoint/read` 可通过 `checkpointId` 读取 snapshot manifest，也可以用
  绑定的 runtime turn id 解析 snapshot。
- `includePatch=true` 返回 snapshot 的 unified diff patch；不执行 restore。

## 验证

- `cargo test acp_initialize_advertises_baseline_agent`
- `cargo test acp_session_checkpoints_lists_loaded_thread_snapshots`
- `cargo test acp_checkpoint_read_returns_manifest_and_patch_by_turn_id`
