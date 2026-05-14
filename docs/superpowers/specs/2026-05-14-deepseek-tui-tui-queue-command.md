# DeepSeek-TUI Parity: TUI Queue Command

## Context

DeepSeek-TUI exposes `/queue [list|edit <n>|drop <n>|clear]` for managing follow-up messages entered while a turn is busy. DeepSeekCode already had durable runtime turns, but focused composer input went straight to `SubmitUserMessage` and had no visible queue management surface.

## Goals

- Add `queue` / `/queue` and `queued` / `/queued` command palette and composer support.
- List queued follow-up messages and any draft currently being edited.
- Edit a queued message by 1-based index by moving it into the composer.
- Drop one queued message by 1-based index.
- Clear queued messages and any queued edit draft.
- Automatically queue normal composer input while the active assistant message is still running.
- Dispatch the next queued message when the active assistant item transitions from running to idle.

## Design

The queue is local TUI session state, matching DeepSeek-TUI's interactive queue semantics rather than durable workspace files. Each queued message records its target thread id and content. The right-side detail panel renders queue state, previews, and command hints.

`TuiApp` detects busy state from the active running assistant message. Normal composer input during that state is parked in `queued_messages` instead of becoming `TuiAction::SubmitUserMessage`. Runtime refresh/live updates detect the running-to-idle transition and enqueue the next queued message as the same existing submit action, so the local and HTTP action handlers do not need a new backend command.

## Acceptance

- `/queue` and `queue list` render the queue detail panel.
- `/queue edit <n>` moves a queued message into the composer and keeps the original target thread.
- `/queue drop <n>` removes a queued message.
- `/queue clear` clears queued messages and any edit draft.
- Normal composer input while an assistant item is `running` does not submit immediately.
- When that running assistant item becomes non-running, the first queued message becomes `TuiAction::SubmitUserMessage`.
- Tests cover list/edit/drop/clear behavior and busy-turn automatic queue dispatch.
