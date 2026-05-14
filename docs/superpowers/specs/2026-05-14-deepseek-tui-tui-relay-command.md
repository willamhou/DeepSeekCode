# DeepSeek-TUI Parity: TUI Relay Command

## Context

DeepSeek-TUI exposes `/relay [focus]` with `/batonpass` and `/接力` aliases.
The command asks the active model to write a compact handoff artifact for a
future thread. DeepSeekCode already has durable sessions, tasks, goals, and
thread context, but it lacked the user-facing relay slash command.

## Goals

- Add `relay [focus]` / `/relay [focus]` command palette and composer support.
- Add `batonpass` / `/batonpass` and `接力` / `/接力` aliases.
- Add relay help rendering in the right-side detail panel.
- Route relay creation through the existing active-thread `SubmitUserMessage`
  action so local and HTTP runtime TUI sessions behave consistently.
- Target `.dscode/handoff.md`, DeepSeekCode's equivalent of DeepSeek-TUI's
  `.deepseek/handoff.md`.
- Include selected workspace, session/thread, TUI mode, optional focus, goal,
  token budget, and active-thread task summaries in the relay instruction.

## Design

`TuiRelayCommand` parses either a create request with optional focus text or a
help request. Create requests do not write files directly from the UI. Instead,
they queue a model instruction asking the active agent to inspect current
context and write a compact Markdown relay:

```text
.dscode/handoff.md
# Session relay
```

This follows DeepSeek-TUI's model-mediated handoff pattern while preserving
DeepSeekCode's `.dscode` project-state namespace.

## Acceptance

- `/relay` queues an active-thread message that asks for `.dscode/handoff.md`.
- `/relay <focus>` includes the requested relay focus.
- `/batonpass <focus>` and `/接力 <focus>` behave as aliases.
- `/relay help` and `/batonpass help` render usage, aliases, active thread, and
  selected workspace.
- The generated prompt includes active goal and task context when present.
- Tests cover command routing, bilingual aliases, prompt contents, and help
  detail rendering.
