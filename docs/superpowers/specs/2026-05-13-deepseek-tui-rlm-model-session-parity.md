# DeepSeek-TUI RLM Model Session Parity

Date: 2026-05-13

Status: implemented

## Gap

DeepSeekCode already exposes RLM one-shot analysis, RLM aliases, batch helpers,
chunk/map-reduce/recursive planners, restricted Python helpers, stateful Python
sessions, and persistent local Python REPL processes. The remaining
DeepSeek-TUI RLM process gap was that model-backed `rlm_process` calls were
bounded child-agent calls with no durable process-style memory across repeated
long-input requests.

## Spec

- `rlm`, `rlm_query`, `llm_query`, and `rlm_process` accept optional
  `session_id` and `reset` fields when invoked with `task` plus `file_path` or
  `content`.
- `session_id` uses a filesystem-safe id: 1-64 chars from
  `[A-Za-z0-9_.-]`, no leading dot, and no `..`.
- Session manifests are stored under `.dscode/rlm-model/<session_id>.json`.
- Each completed model-backed process call appends task, input source, input
  size, output summary, and update time to the session.
- Later calls with the same `session_id` inject the most recent bounded session
  summaries into the child-agent prompt before the new long input.
- `reset=true` clears the loaded session before the current call.
- Session history is bounded to avoid unbounded prompt and disk growth.

## Implementation

- Added durable `RlmModelSession` read/write helpers in `src/tools/rlm.rs`.
- Added prior-session rendering for `rlm_process` child-agent prompts.
- Added metadata lines to session-backed outputs:
  - `meta.rlm_session_id=<id>`
  - `meta.rlm_session_turns=<count>`
- Updated DeepSeek model tool schemas so OpenAI/Anthropic tool definitions
  expose `session_id` and `reset`.
- Updated runtime and parity docs to record the narrowed gap.

## Verification

- `cargo test rlm_model_session --lib`
- `cargo test rlm_process --lib`
- `cargo test build_tool_specs_include_rlm --lib`
- `cargo test serve --lib`
- `cargo fmt --check`
- `cargo check`
- `git diff --check`

## Remaining Gap

This is not a true live model REPL or daemon. It persists bounded summaries
around bounded child-agent calls, which gives durable process-style context for
common repeated RLM workflows. Full parity with a continuously live
model-backed process would still require durable runtime threads, resumable
model state, stronger cancellation/ownership semantics, and recovery behavior
after the owning DeepSeekCode process exits.
