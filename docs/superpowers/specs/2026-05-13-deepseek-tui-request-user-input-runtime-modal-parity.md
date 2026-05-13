# DeepSeek-TUI Request User Input Runtime Modal Parity Spec

日期：2026-05-13

对比对象：`Hmbown/DeepSeek-TUI`，`main` HEAD `3382242`

## 背景

DeepSeekCode already exposes a DeepSeek-TUI-compatible `request_user_input`
tool that validates 1-3 structured questions and returns
`meta.user_input_required=true`. The remaining gap is runtime/TUI handling:
there is no durable request event, no TUI modal, and no structured response
event for external clients or a future blocking agent bridge.

## 目标

- Add durable runtime `user_input_request` events carrying validated questions.
- Add durable runtime `user_input_response` events carrying selected answers.
- Allow `POST /v1/threads/{id}/events` to append both event kinds.
- Load unresolved user-input requests into local and remote TUI runtime
  snapshots.
- Render a TUI modal for pending user-input requests.
- Let users choose options with number keys and record a structured
  `RespondUserInput` action.

## 非目标

- This slice does not yet block/resume the agent loop inside
  `request_user_input`.
- This slice does not implement free-form "Other" text entry.
- This slice does not add a multi-process waiter for tool execution.

## 验收标准

1. Runtime store can append and read `user_input_request` /
   `user_input_response` events.
2. HTTP runtime event endpoint accepts both new event kinds.
3. TUI snapshots hide requests that already have a matching response.
4. TUI opens a user-input modal for unresolved requests.
5. Number keys select options and emit `TuiAction::RespondUserInput`.
6. Runtime docs and parity plan describe the remaining non-blocking boundary.

## 实现结果

- Added `RuntimeStore::append_user_input_request` with validation for 1-3
  questions and 2-3 options.
- Added `RuntimeStore::append_user_input_response` with structured answer maps.
- Extended `POST /v1/threads/{id}/events` to accept `user_input_request` and
  `user_input_response`.
- Local and remote TUI runtime snapshots now collect unresolved
  `user_input_request` events and hide requests that already have a matching
  response.
- Added `TuiUserInputRequest` / question / option runtime event adapters.
- Added a TUI user-input modal that opens for pending requests, shows one
  question at a time, and uses `1` / `2` / `3` to select labeled options.
- Added `TuiAction::RespondUserInput`, with local and remote runtime handlers
  that append durable response events.
- Updated runtime docs and the DeepSeek-TUI parity plan with the remaining
  non-blocking boundary.

## 验证

- `cargo test user_input`: passed, 10 tests.
- `cargo test replace_runtime_with_user_input_opens_modal_and_records_answer`:
  passed.
- `cargo test app_from_store_hides_answered_user_input_events`: passed.
- `cargo test handle_tui_action_records_user_input_response`: passed.
- `cargo test append_user_input_request_and_response_records_events`: passed.
- `cargo test event_endpoint_appends_user_input_events`: passed.
- `cargo test`: passed, 1024 tests.
- `cargo fmt --check`: passed.
- `git diff --check`: passed.
- `cargo package --allow-dirty`: passed, packaged 303 files.
