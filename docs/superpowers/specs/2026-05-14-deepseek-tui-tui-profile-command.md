# DeepSeek-TUI Parity: TUI Profile Command

## Context

DeepSeek-TUI exposes `/profile <name>` for hot-switching named config profiles.
DeepSeekCode already has project config commands for model/provider settings, but
it did not have a named profile selector or config-profile overlay.

## Goals

- Support named profile definitions in project config using `profiles.<name>.*`
  flat keys or `[profiles.name]` TOML-style sections.
- Add `workspace.active_profile` as the persisted selected profile.
- Add TUI `profile` / `/profile` commands:
  - `profile` / `/profile` shows active profile and configured profiles.
  - `profile list` / `/profile list` lists configured profiles.
  - `profile <name>` / `/profile <name>` persists the selected profile.
  - `profile clear` / `/profile clear` clears the selected profile.
- Apply the selected profile before environment variable overrides in
  `load_or_default()`, so later local TUI turns use the selected profile.

## Acceptance

- Config parsing overlays selected profile values onto base config.
- Missing selected profiles fail with a clear error.
- TUI queues profile commands before custom slash fallback.
- Local TUI action persists `workspace.active_profile` and renders a Profile
  detail panel.
- Tests cover config profile overlay, command parsing/queuing, and local action
  handling.
