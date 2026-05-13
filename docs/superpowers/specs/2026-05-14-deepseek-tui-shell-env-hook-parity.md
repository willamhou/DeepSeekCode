# DeepSeek-TUI Shell Env Hook Parity

Status: implemented

## Gap

DeepSeek-TUI supports a `shell_env` hook that runs immediately before shell tool
execution and parses stdout `KEY=VALUE` lines into the spawned command
environment. DeepSeekCode already had local hook directories for session,
prompt, tool, permission, subagent, and compaction events, but shell tools could
not receive ephemeral per-command environment values from hooks.

## Implementation

- Added a `shell_env` hook event and bootstrap directory under
  `.dscode/hooks/shell_env`.
- `HookRunner::shell_env` runs user hooks first, then project hooks, and parses
  `KEY=VALUE` / `export KEY=VALUE` stdout lines into per-call environment
  variables.
- AgentLoop applies `shell_env` only to `run_shell`, `exec_shell`, and
  `task_shell_start`, immediately before execution and after approval checks.
- Shell env values are injected through hidden `env.<KEY>` tool-input keys.
  Durable tool-event input keeps the original model arguments, so secret values
  are not recorded in runtime tool inputs.
- `run_shell`, `exec_shell`, foreground-detach `exec_shell`, and background
  shell jobs now apply hidden env args to their spawned processes.
- Docs now list the `shell_env` hook directory and describe the value-redaction
  behavior.

## Verification

- `cargo test shell_env --lib`
- `cargo test run_shell_applies_hidden_env_args --lib`
- `cargo test exec_shell_background_applies_hidden_env_args --lib`
- `cargo test run_with_client_applies_shell_env_hook_without_recording_secret_input --lib`
- `cargo test init_config_creates_project_bootstrap_files --lib`
- `cargo test hooks --lib`
- `cargo test run_shell --lib`
- `cargo test exec_shell --lib`
- `cargo test loop_runtime --lib`
- `cargo fmt --check`
- `cargo check`
- `git diff --check`

## Remaining

This covers DeepSeek-TUI-style per-shell env injection for the local agent loop.
Direct MCP/ACP shell tool calls currently do not run local hooks because those
surfaces are runtime-approval driven rather than AgentLoop hook driven.
