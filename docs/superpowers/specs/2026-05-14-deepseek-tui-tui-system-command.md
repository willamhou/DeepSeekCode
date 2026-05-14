# DeepSeek-TUI Parity: TUI System Command

## Context

DeepSeek-TUI exposes `/system` as a debug command that shows the current system
prompt. DeepSeekCode builds its runtime system prompt inside the local agent
loop and does not persist that prompt into the durable TUI snapshot, so the TUI
needs a local read-only preview path.

## Goals

- Add `system` / `/system` command-palette and composer support.
- Add `system help` / `/system help` detail rendering.
- Reuse the existing local runtime system-prompt builder instead of duplicating
  prompt text in the TUI.
- Include selected workspace instructions and configured user memory in the
  preview, matching the local agent loop's prompt inputs.
- Use the selected thread's latest user message as task context when available
  so skill, research-bootstrap, and planning-mode heuristics are visible.
- Render the prompt in the right-side detail panel without starting a model turn
  or mutating durable runtime state.
- Reject unsupported arguments with a concise usage error.

## Design

`TuiApp` parses `system` / `/system` into a local `TuiAction::ShowSystemPrompt`
with the selected workspace, current UI mode, and latest selected user message.
The local file-backed TUI handler calls
`preview_system_prompt_for_workspace`, which loads workspace instructions, user
memory, local skills, and tool policy before building the same prompt string the
agent loop would use. The HTTP runtime path reports that this preview requires a
local file-backed TUI because remote snapshots do not expose assembled prompt
text.

## Acceptance

- `system` / `/system` queues a local system prompt preview action.
- The action renders a `System Prompt` detail panel with workspace, profile,
  task, planning, skill, workspace-instruction, user-memory, and prompt content.
- `system help` / `/system help` renders usage details.
- Invalid arguments show
  `usage: system or /system; use system help for details`.
- Tests cover TUI command routing, help rendering, latest-user-message
  selection, local handler rendering, and core prompt-preview assembly.
