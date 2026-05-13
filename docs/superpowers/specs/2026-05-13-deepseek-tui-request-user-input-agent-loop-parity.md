# DeepSeek-TUI Request User Input Agent Loop Parity Spec

日期：2026-05-13

对比对象：`Hmbown/DeepSeek-TUI`，`main` HEAD `3382242`

## 背景

DeepSeekCode has a model-visible `request_user_input` tool plus durable
runtime `user_input_request` / `user_input_response` events and a TUI modal.
The remaining parity gap is that runtime-backed agent runs still treat the
tool as a non-blocking summary instead of pausing until the user answers.

## 目标

- Add an agent-loop user-input resolver hook, parallel to durable approval
  resolution.
- When `request_user_input` runs under a runtime-backed resolver, append a
  durable `user_input_request` event and wait for the matching
  `user_input_response`.
- Return the selected answers as the tool observation so the next model step
  can continue with the user's choices.
- Wire the resolver into TUI-started agent runs.
- Wire the resolver into daemon/runtime task runs so external TUI or HTTP
  clients can answer background clarifications.

## 非目标

- Free-form "Other" text entry was out of scope for this slice; a later TUI
  modal slice added short Other answers submitted through the same durable
  response event.
- This slice does not change the existing non-runtime CLI fallback summary.
- This slice does not add a new HTTP endpoint beyond the existing runtime
  event append endpoint.

## 验收标准

1. `AgentLoopOptions` can carry a user-input resolver.
2. Runtime-backed `request_user_input` calls block until a matching response
   event arrives.
3. The returned tool output includes a structured answer map.
4. TUI-started agent runs use the blocking resolver.
5. Runtime task runs use the blocking resolver.
6. Focused tests cover core agent-loop resolution and runtime-backed resolver
   waiting.

## 实现结果

- Added `AgentUserInputRequest`, `AgentUserInputResponse`, and
  `AgentUserInputResolver` to the agent loop options.
- Runtime-backed `request_user_input` calls now use the resolver path and
  return `meta.user_input_required=false` plus `answers_json`.
- TUI-started agent turns append durable user-input requests, wait for matching
  responses, and honor runtime cancellation while waiting.
- Runtime task / daemon agent runs append durable user-input requests and wait
  for external TUI or HTTP responses.
- Runtime docs and the DeepSeek-TUI parity plan now describe the completed
  blocking runtime-backed path and the remaining plain CLI fallback behavior.

## 验证

- `cargo test run_with_client_uses_user_input_resolver_for_request_user_input`:
  passed.
- `cargo test runtime_user_input_resolver_waits_for_response_event`: passed.
- `cargo test runtime_task_user_input_resolver_waits_for_durable_response`:
  passed.
- `cargo test user_input`: passed, 13 tests.
- `cargo test`: passed, 1027 tests.
- `cargo fmt --check`: passed.
- `git diff --check`: passed.
- `cargo package --allow-dirty`: passed, packaged 304 files.
