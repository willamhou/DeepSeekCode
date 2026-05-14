# TUI Live Tool Run Events

## Context

DeepSeek-TUI recently tightened its live tool status surface so long-running
shell and CI polling work stays visible without noisy wrapper commands. In
DeepSeekCode, TUI-started agent runs already stream assistant text and
reasoning items, but tool calls were only persisted as `tool_result` items
after the whole model turn finished.

## Spec

- Add a TUI run-events bridge for TUI-started agent runs.
- Persist a `tool_call` item when a tool starts, with `running` status.
- Update that `tool_call` item to `pending` when the tool requires approval.
- Update the active `tool_call` item to `completed` or `failed` when the tool
  result arrives.
- Persist and live-upsert the matching `tool_result` item as soon as the tool
  finishes.
- Keep final turn recording from writing duplicate `tool_result` items that
  were already persisted live, while still recording late post-processing tool
  events such as posthoc translation.
- Use concise live targets for shell wrappers such as
  `cd ... && sleep ... && gh pr checks ...`.

## Verification

- `runtime_tool_run_events_persist_and_emit_live_tool_updates`
- `record_tui_agent_result_skips_live_persisted_tool_results`
- `cargo fmt --check`
- `cargo test runtime_tool_run_events_persist_and_emit_live_tool_updates --lib`
- `cargo test record_tui_agent_result_skips_live_persisted_tool_results --lib`
- `cargo test runtime_item_stream --lib`
- `cargo check`
- `cargo test --lib -- --test-threads=1`
- `git diff --check`
