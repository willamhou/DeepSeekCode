# DeepSeek-TUI Parity: TUI Subagents Command

## Context

DeepSeek-TUI exposes `/subagents` and `/agents` for sub-agent visibility, plus
`/agent [N] <task>` for starting a persistent sub-agent flow with a bounded
recursive depth. DeepSeekCode already has runtime-backed sub-agent lifecycle
tools (`agent_spawn`, `send_input`, `agent_list`, `resume_agent`,
`close_agent`), but the TUI command palette did not yet expose the direct
DeepSeek-TUI-style commands.

## Goals

- Add `subagents` / `/subagents` and `agents` / `/agents` command palette and
  composer support.
- Add `agent [0-3] <task>` / `/agent [0-3] <task>` for queueing a persistent
  active-thread sub-agent task.
- Default omitted `/agent` depth to `1`; reject depth values outside `0..=3`.
- Render active-thread `subagent` and `subagent_input` task records in the
  right-side detail panel.
- Queue local file-backed `subagent` runtime tasks with `status=pending` and a
  summary that preserves the requested depth.
- In HTTP-runtime TUI mode, post the same pending `subagent` task payload to
  `/v1/threads/{thread_id}/tasks`.

## Design

`TuiSubagentsCommand` separates read-only list/help behavior from spawn
requests. The UI layer renders list/help details locally from the active TUI
snapshot and emits `TuiAction::CreateSubagentTask` only for `agent` commands.

The action handler writes a durable runtime task:

```text
kind=subagent
status=pending
summary=max_depth=N: <task>
```

This keeps the TUI command compatible with existing daemon and external runner
flows that already consume pending runtime tasks, while matching
DeepSeek-TUI's user-facing depth syntax.

## Acceptance

- `/subagents` and `/agents` render only active-thread sub-agent task records.
- `/agent <task>` queues a pending sub-agent task with default depth `1`.
- `/agent 2 <task>` queues a pending sub-agent task with depth `2`.
- `/agent 4 <task>` is rejected with a depth-range error.
- `/agent help` and `/agents help` render command usage in the detail panel.
- Local file-backed TUI persists the pending `subagent` task and emits the
  normal runtime task-record event.
- HTTP-runtime TUI posts a pending `subagent` task to the selected remote
  thread.
- Tests cover command routing, list rendering, local task creation, and the
  existing HTTP compile path for remote task creation.
