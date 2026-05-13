# DeepSeek-TUI ACP Checkpoint Restore Parity Spec

日期：2026-05-12

对比对象：`Hmbown/DeepSeek-TUI`，`main` HEAD `3382242`

## 背景

上一轮已经让 ACP adapter 能发现和读取 rollback checkpoints，但外部 ACP
client 仍无法执行 DeepSeekCode 已有的 `restore revert-turn` dry-run / apply
能力。DeepSeek-TUI 的 ACP baseline 本身不覆盖 checkpoint restore；这里按
Claude/Codex 风格 editor-agent 期望继续扩展 ACP runtime bridge。

## 目标

- ACP `initialize` 声明 checkpoint restore/apply 能力，不再标成只读。
- 新增 `session/checkpoint/restore`。
- `session/checkpoint/restore` 默认 dry-run，不写 worktree。
- 只有显式传入 `apply=true` 才调用 rollback restore 写回 git worktree。
- 支持通过 snapshot id 或绑定的 runtime turn id restore。
- 当传入 loaded ACP `sessionId` 或 `threadId` 时，restore 只允许作用于该 runtime
  thread 绑定的 checkpoint。
- 返回结构化 restore plan，包含 applied、patch byte counts、current diff byte
  count、changed files 等字段。

## 非目标

- 不新增通用 ACP file-write/shell tools。
- 不改变 rollback snapshot 存储格式。
- 不在 ACP restore path 里运行 post-restore diagnostics；CLI restore 仍保留该输出。
- 不绕过 git `HEAD` 一致性校验。

## 验收标准

1. `initialize` response 包含 `sessionCapabilities.checkpoints.readOnly=false`、
   `restore=true` 和 `apply=true`。
2. `session/checkpoint/restore` 不带 `apply` 时返回 dry-run plan，且不改文件。
3. `session/checkpoint/restore` 带 `apply=true` 时恢复 snapshot 捕获的 dirty
   worktree 状态。
4. restore 支持通过绑定的 runtime turn id 定位 snapshot。
5. loaded ACP session restore 会按 runtime thread 过滤 checkpoint。

## 实现结果

- `serve --acp` 新增 `session/checkpoint/restore` dispatch。
- `initialize` 的 checkpoint capability 从 read-only 改为
  `readOnly=false, restore=true, apply=true`。
- restore 默认 dry-run；`apply=true` 时复用 `RollbackStore::restore_snapshot`。
- 传入 `sessionId` / `threadId` 时，先校验 checkpoint 的
  `runtime_thread_id` 归属。
- 返回 `checkpoint` manifest、`restore` plan 和 `mode`。

## 验证

- `cargo test acp_initialize_advertises_baseline_agent`
- `cargo test acp_checkpoint_restore_dry_run_does_not_mutate_worktree`
- `cargo test acp_checkpoint_restore_apply_restores_loaded_session_turn`
