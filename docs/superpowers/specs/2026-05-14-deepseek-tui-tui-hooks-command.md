# DeepSeek-TUI Parity: TUI Hooks Command

## Context

DeepSeek-TUI exposes a read-only `/hooks` slash command that helps users discover configured lifecycle hooks and valid event names without reading source. DeepSeekCode already has local hook execution, including `shell_env`, but the TUI did not expose hook inventory.

## Goals

- Add `hooks` / `/hooks` command palette and composer support.
- Support `hooks list` / `/hooks list` as the default action.
- Support `hooks events` / `/hooks events` for supported event discovery.
- Render hook details in the TUI detail panel without executing hook scripts.
- Keep HTTP runtime TUI fail-closed for local-only filesystem inspection.

## Design

DeepSeekCode hooks are directory-based instead of DeepSeek-TUI's TOML hook-list model. The TUI command therefore lists:

- global enabled state and timeout
- configured project and user hook roots
- each supported event directory
- executable scripts that will fire
- non-executable files or non-file entries that are ignored

Supported events come from `HookEvent::dir_name()`:

- `session_start`
- `session_stop`
- `user_prompt_submit`
- `pre_tool_use`
- `permission_request`
- `post_tool_use`
- `subagent_start`
- `subagent_stop`
- `pre_compact`
- `shell_env`

## Acceptance

- `hooks`, `hooks list`, `/hooks`, and `/hooks list` queue a local `TuiAction::Hooks { List }`.
- `hooks events`, `hook events`, `/hooks events`, and `/hook events` queue `TuiAction::Hooks { Events }`.
- Local file-backed TUI renders hook inventory from `config.hooks`.
- Remote HTTP runtime TUI reports that hooks commands require local file-backed TUI.
- Tests cover command routing and local inventory/event rendering.
