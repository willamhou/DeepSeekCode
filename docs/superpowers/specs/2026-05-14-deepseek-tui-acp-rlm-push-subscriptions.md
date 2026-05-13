# DeepSeek-TUI ACP RLM Push Subscriptions

Status: implemented

## Gap

HTTP clients can follow aggregate runtime SSE and receive mirrored
`rlm_live_event` records, but ACP clients previously had no ACP-native way to
receive those live RLM updates through `session/update`. That kept the final
RLM subscription surface split between HTTP/TUI clients and ACP clients.

## Implementation

- `initialize` now advertises `sessionCapabilities.rlmLiveEvents.subscribe`
  with `cursor = "runtime_event_seq"`.
- `session/rlm/subscribe` is a DeepSeekCode ACP extension for sessions loaded
  from a runtime thread.
- The method accepts `cursor` / `sinceSeq`, `limit`, `waitMs`, and `pollMs`.
- It reads runtime events from the loaded thread, filters `kind=rlm_live_event`,
  emits ACP `session/update` notifications using standard `tool_call` /
  `tool_call_update` payloads, and returns `nextCursor`.
- Updates include the original runtime event and live RLM event under
  `rawOutput`, with `_meta.deepseek.kind =
  "deepseek.acp.rlm_live_event.v1"` and `_meta.runtime` ids for audit
  alignment.

## Verification

- `cargo test acp_session_rlm_subscribe --lib`
- `cargo test acp_initialize_advertises_baseline_agent --lib`
- `cargo test acp_ --lib`
- `cargo test serve --lib`
- `cargo fmt --check`
- `cargo check`
- `git diff --check`

## Remaining

No open first-order MCP/ACP RLM subscription gap remains. Published npm package
and Homebrew tap distribution are still tracked under the broader packaging
phase.
