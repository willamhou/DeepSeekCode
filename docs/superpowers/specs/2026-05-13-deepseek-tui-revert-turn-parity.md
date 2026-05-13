# DeepSeek-TUI Revert Turn Parity Spec

日期：2026-05-13

对比对象：`Hmbown/DeepSeek-TUI`，`main` HEAD `3382242`

## 背景

DeepSeek-TUI exposes an agent-callable `revert_turn` tool that restores
workspace files from the snapshot captured before a recent turn. This is used
when the user explicitly asks to undo or roll back edits.

DeepSeekCode already has rollback snapshots, CLI restore commands, TUI
rollback actions, and ACP checkpoint restore. The missing compatibility layer
was the direct model tool name and schema.

## 目标

- Add an agent-visible `revert_turn` tool.
- Allow restore by `snapshot_id` / `checkpoint_id` / `id`, by `turn_id`, or by
  recent `turn_offset`.
- Require the existing write confirmation/approval path because restoring a
  snapshot mutates workspace files.
- Expose `revert_turn` in model schemas.
- Expose `revert_turn` through MCP/ACP only when durable runtime approvals are
  enabled.
- Support `dry_run=true` / `apply=false` preview mode.

## 非目标

- This slice does not change how rollback snapshots are captured.
- This slice does not modify conversation history when restoring files.
- This slice does not bypass the current HEAD safety check in `RollbackStore`.

## 验收标准

1. `revert_turn snapshot_id=<id>` restores files from the selected rollback
   snapshot.
2. `revert_turn dry_run=true` reports the restore plan without mutating files.
3. `turn_offset` selects recent runtime-turn snapshots, optionally scoped by
   `thread_id`.
4. Registry permission requests classify `revert_turn` as a write operation.
5. OpenAI/Anthropic tool schemas include `revert_turn`.
6. MCP/ACP list `revert_turn` only when durable approvals are enabled, and MCP
   execution waits for a runtime permission response.

## 实现结果

- `src/tools/revert_turn.rs` adds `RevertTurnTool` on top of
  `core::rollback::RollbackStore`.
- `src/tools/registry.rs` registers `revert_turn` and routes it through the
  existing write confirmation path.
- `src/model/deepseek.rs` exposes the `revert_turn` schema.
- `src/cli/commands/serve.rs` exposes MCP/ACP `revert_turn` behind durable
  approvals.

## 验证

- `cargo test revert_turn`
- `cargo test build_tool_specs_include_revert_turn`
- `cargo test default_registry_includes_read_only_git_history_tools`
- `cargo test mcp_tools_call_executes_revert_turn_after_runtime_approval`
- `cargo test mcp_tools_list_includes_write_tools_only_with_durable_approvals`
- `cargo test acp_session_tools_list_new_session_is_read_only`
- `cargo fmt --check`
- `git diff --check`
- `cargo test`（919 passed）
- `cargo package --allow-dirty`
