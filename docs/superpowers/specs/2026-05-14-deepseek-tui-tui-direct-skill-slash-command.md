# DeepSeek-TUI Parity: Direct Skill Slash Commands

## Context

DeepSeek-TUI lets configured skills participate in the slash-command namespace:
after native and user-configured slash commands, `/<skill-name>` is treated as
an exact skill name. DeepSeekCode currently supports `skill <name>` /
`/skill <name>`, but `/<skill-name>` falls through to the custom slash missing
path.

## Goals

- Keep native commands and project/user custom slash commands higher priority.
- When a custom slash command is missing, check whether the command name matches
  a configured repo or user skill.
- If it matches and no arguments were supplied, show the same skill detail as
  `/skill <name>` instead of reporting a missing custom slash command.
- Add direct `/<skill-name>` slash completions for configured skills alongside
  existing `/skill <name>` completions.
- Update TUI docs and parity plan.

## Acceptance

- With a configured `pr-review` skill and no custom `/pr-review` command,
  `RunCustomSlashCommand { command: "/pr-review" }` renders the skill detail and
  sets status `skill shown: pr-review`.
- A truly missing slash command still reports `custom slash command not found`.
- Slash completions include both `/skill pr-review` and `/pr-review`.
- Full `tui` tests continue passing.
