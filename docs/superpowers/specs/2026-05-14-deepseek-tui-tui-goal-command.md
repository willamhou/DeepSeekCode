# DeepSeek-TUI Parity: TUI Goal Command

## Context

DeepSeek-TUI exposes `/goal` for setting a session objective and optional token budget. DeepSeekCode already has durable tasks and telemetry, but the TUI lacked this lightweight objective display.

2026-05-24 update: DeepSeekCode now stores TUI goals on the active runtime
thread instead of only in process memory. The command still behaves like a
lightweight objective in the UI, but file-backed and HTTP-runtime TUI sessions
restore it through durable `thread_goal_set` / `thread_goal_cleared` events.

## Goals

- Add `goal` / `/goal` command palette and composer support.
- Support showing the current goal when no argument is provided.
- Support setting a goal with optional `budget: N` token budget syntax.
- Support `goal clear`, `goal reset`, and `goal done`.
- Render goal details in the right-side TUI detail panel.
- Persist active-thread goal state through the runtime when an active thread is
  selected.

## Design

The goal is keyed by active runtime thread. `TuiApp` keeps an in-memory map for
fast UI rendering, and file-backed or HTTP runtime sessions persist changes as
append-only `thread_goal_set` and `thread_goal_cleared` events. This keeps
`goal` / `/goal` available after TUI restart, thread reselection, process rerun,
and runtime reconnect.

When no active runtime thread is selected, the command can still render the
local empty state, but set/clear persistence requires a thread id. Goal updates
do not mutate user project files.

The detail panel shows:

- objective
- elapsed time since it was set
- optional token budget
- active-thread cumulative token usage and percentage when usage telemetry exists
- command reminders for show, replace, and clear

## Acceptance

- `goal <objective> [budget: N]` and `/goal <objective> [budget: N]` set the active-thread goal.
- `goal` and `/goal` show the current active-thread goal or an empty-state prompt.
- `goal clear`, `goal reset`, `goal done`, and slash equivalents clear the active-thread goal.
- Existing active-thread usage summaries are used for token budget progress.
- Tests cover setting, showing, budget rendering, composer-based clearing,
  runtime event round-tripping, TUI action persistence, and goal restoration when
  a thread is selected.
