# DeepSeek-TUI RLM Live Session Inventory

Date: 2026-05-13

Status: implemented

## Gap

The live RLM daemon design now defines `.dscode/rlm-daemon/<session_id>/`
manifests, but `rlm_process_sessions` only listed legacy
`.dscode/rlm-model/<session_id>.json` summary sessions. That meant the first
live daemon implementation slice would have no model-visible inventory surface.

## Spec

- Keep default `rlm_process_sessions` behavior unchanged for legacy durable
  model-session summaries.
- Add `include_live=true` to list live RLM daemon manifests from
  `.dscode/rlm-daemon/<session_id>/manifest.json`.
- Validate live session ids with the same safe id rules used by
  `rlm_process` model sessions.
- Return compact live inventory fields: status, daemon pid, runtime thread id,
  active turn id, queued turn count, update time, and last error.
- When inspecting a specific `session_id` with `include_live=true`, include the
  normalized live manifest if present, while still returning the legacy summary
  session information.
- Do not start models, enqueue turns, or claim live daemon execution yet.

## Implementation

- Added live-session path helpers for `.dscode/rlm-daemon`.
- Added tolerant manifest parsing and normalized
  `deepseek.rlm.live_session.v1` output.
- Extended `rlm_process_sessions` with `include_live=true` for list and inspect
  modes.
- Updated OpenAI-format model tool schemas and MCP tool definitions.
- Added a regression test for list and inspect behavior.

## Verification

- `/home/willamhou/.cargo/bin/cargo test rlm_process_sessions_can_include_live_daemon_manifests --lib`
- `/home/willamhou/.cargo/bin/cargo test rlm_process_sessions --lib`
- `/home/willamhou/.cargo/bin/cargo test rlm_process --lib`
- `/home/willamhou/.cargo/bin/cargo test rlm_model_session --lib`
- `/home/willamhou/.cargo/bin/cargo test build_tool_specs_include_rlm --lib`
- `/home/willamhou/.cargo/bin/cargo test serve --lib`
- `/home/willamhou/.cargo/bin/cargo fmt --check`
- `/home/willamhou/.cargo/bin/cargo check`
- `git diff --check`

## Remaining Gap

This implements the first live RLM daemon design slice: manifest and inventory.
It does not create a daemon, enqueue live turns, stream model deltas, cancel
active live turns, or recover interrupted worker state. Those remain in the
runtime-thread-backed queue, streaming/cancellation, and recovery slices.
