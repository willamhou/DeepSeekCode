# DeepSeek-TUI Parity: TUI Task Slash Command

## Context

DeepSeek-TUI exposes `/task [add <prompt>|list|show <id>|cancel <id>]`.
DeepSeekCode already has a task panel and non-slash command-palette task
commands, but focused-composer or command-palette `/task ...` input can still
fall through to custom slash command handling.

## Goals

- Route `/task` and `/tasks` before custom slash fallback.
- Support `/task list`, `/task add <prompt>`, `/task show <id>`, and
  `/task cancel <id>`.
- Preserve DeepSeekCode's existing task creation/cancel behavior and active
  thread task panel.
- Add slash completions and update TUI docs/plan.

## Acceptance

- `/task add inspect logs` queues `TuiAction::CreateTask`.
- `/task list` surfaces active-thread task status instead of queuing custom
  slash.
- `/task show <id>` renders task details.
- `/task cancel <id>` reuses the existing task cancellation flow.
- Full `tui` tests continue passing.
