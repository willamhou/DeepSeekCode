# DeepSeek-TUI RLM Live Turn Queue

Date: 2026-05-13

Status: implemented

## Gap

Live RLM daemon manifests were discoverable through
`rlm_process_sessions include_live=true`, but `rlm_process` still had no way to
create a live session or enqueue a turn onto a durable runtime thread without
running the bounded child-agent adapter immediately.

## Spec

- Add `live=true` to `rlm_process` and aliases in task mode.
- Require `session_id` for live mode.
- For a new live session, require `file_path` or `content` so the first turn has
  concrete long input.
- For an existing live session, allow `task + session_id + live=true` without
  fresh input and mark it as a session-context-only turn.
- Create or reuse a runtime thread for the live session.
- Enqueue each live turn as a pending `rlm_process` runtime task.
- Write `.dscode/rlm-daemon/<session_id>/manifest.json` with runtime thread id,
  status, queued turn count, model, workspace, and timestamps.
- Append `turn_queued` records to
  `.dscode/rlm-daemon/<session_id>/events.jsonl`.
- Return metadata that lets clients poll inventory without spending model
  tokens.

## Implementation

- Added `live=true` routing before the bounded child-agent execution path.
- Added live-session manifest writer and event-log appender.
- Reused `RuntimeStore` to create/reuse the live session runtime thread and
  write pending runtime tasks.
- Updated model tool schemas and MCP tool definitions with the `live` option.
- Added a regression test that enqueues two live turns, verifies the manifest,
  runtime tasks, and event log, and confirms no model execution is required.

## Verification

- `/home/willamhou/.cargo/bin/cargo test rlm_process_live_enqueues_runtime_turn_and_manifest --lib`
- `/home/willamhou/.cargo/bin/cargo test rlm_process --lib`
- `/home/willamhou/.cargo/bin/cargo test rlm_process_sessions --lib`
- `/home/willamhou/.cargo/bin/cargo test build_tool_specs_include_rlm --lib`
- `/home/willamhou/.cargo/bin/cargo test serve --lib`
- `/home/willamhou/.cargo/bin/cargo fmt --check`
- `/home/willamhou/.cargo/bin/cargo check`
- `git diff --check`

## Remaining Gap

This is durable live-session queueing, not a model worker. There is still no
live RLM daemon process that claims pending turns, streams model deltas, handles
turn cancellation, marks turns complete, or recovers interrupted active turns.
