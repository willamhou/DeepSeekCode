# Hooks Fixture Smoke

Date: 2026-05-23

## Goal

Phase 12D needs local proof that hooks are not just individually unit-tested,
but wired through the actual agent loop. The acceptance target is prompt submit,
pre-tool, post-tool, and session start/stop coverage with structured hook
decisions.

## Command

```bash
deepseek hooks fixture-smoke --json
```

The command is no-network and does not call a live model. It creates a temporary
workspace, installs executable recorder hooks under a temporary hook root, and
runs `AgentLoop::run_with_client` with a deterministic fixture model client. The
fixture client triggers one real `list_files` tool call and then finishes.

## Covered Events

- `session_start`
- `user_prompt_submit`
- `pre_tool_use`
- `post_tool_use`
- `session_stop`

Each recorder hook receives the standard JSON payload on stdin, records
`DSCODE_HOOK_EVENT` plus the payload, and returns structured output:

```json
{"decision":"allow","add_context":"<event> context"}
```

The smoke verifies that lifecycle events were recorded with matching payload
`event` values, that pre/post tool hooks refer to `list_files`, that post-tool
status is `ok`, that hook context observations reach the next model request,
and that the tool output contains the fixture `README.md`.

## JSON Contract

Successful output uses:

```json
{
  "kind": "deepseek.hooks_fixture_smoke.v1",
  "session_start_ok": true,
  "user_prompt_submit_ok": true,
  "pre_tool_ok": true,
  "post_tool_ok": true,
  "session_stop_ok": true,
  "hook_contexts_ok": true,
  "tool_ran_ok": true,
  "events": [
    "session_start",
    "user_prompt_submit",
    "pre_tool_use",
    "post_tool_use",
    "session_stop"
  ]
}
```

## Verification

Current local evidence:

- `cargo test cli::commands::hooks --lib`
- `cargo test cli::app::tests::parses_hooks_fixture_smoke_subcommand --lib`
- `cargo test cli::commands::help::tests::hooks_help_documents_fixture_smoke --lib`
- `cargo run --quiet -- hooks fixture-smoke --json`

The latest CLI smoke reported every lifecycle boolean as `true` and one
successful `list_files` tool event.
