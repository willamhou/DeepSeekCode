# DeepSeek-TUI Parity: TUI Provider Picker

## Context

DeepSeekCode already supports `/provider <name> [model]`, `provider list`, and
`/config provider ...` for local file-backed TUI sessions. That closes the
config mutation path, but `/provider` still only shows current config while
DeepSeek-TUI opens an interactive provider/model selection surface.

## Goals

- Make `provider` / `/provider` open an interactive provider picker from the
  TUI command palette and composer slash command.
- Keep `provider show` / `/provider show` for the existing read-only config
  detail.
- Keep `provider list` / `/provider list` for the provider catalog detail.
- Render a two-pane picker with provider presets and model choices.
- Use keyboard navigation: up/down, left/right or tab, enter to apply, escape to
  close.
- Queue the same `TuiAction::Provider::Set` action used by direct provider
  commands, preserving existing local config mutation behavior.

## Acceptance

- `provider` opens the picker and does not immediately mutate config.
- `/provider show` still queues a provider config detail action.
- `/provider list` still queues a provider catalog action.
- Entering the picker on a selected provider/model queues
  `TuiAction::Provider::Set`.
- The picker renders provider and model panes with the selected workspace action
  preview.
- Existing direct provider updates and `/config provider ...` routing remain
  unchanged.
- Full `tui` tests continue passing.
