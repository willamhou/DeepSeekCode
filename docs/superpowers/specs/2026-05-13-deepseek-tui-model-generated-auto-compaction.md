# DeepSeek-TUI Model-Generated Auto Compaction

## Context

DeepSeekCode already records runtime usage, recommends compaction near the
1M-token context limit, exposes manual `compact` actions in the TUI, and has a
daemon that appends non-destructive runtime compaction records after the latest
usage record crosses the warning threshold.

The remaining Phase E gap is that daemon compaction uses only the deterministic
extractive fallback. DeepSeek-TUI-style long-context operation should preserve
older intent with a model-generated summary when a model key is configured,
while retaining the local fallback for offline or failed summary generation.

## Scope

- Add a daemon compaction summary policy:
  - if the configured model API key environment variable is set, ask the model
    for a concise older-context summary before compacting;
  - if the key is absent or model summary generation fails, keep the existing
    extractive compaction behavior;
  - never block compaction entirely because model summary generation failed.
- Mark model-generated summaries with `summary_source = "model"` in the runtime
  compaction record and daemon JSON event.
- Keep HTTP/manual compaction semantics unchanged:
  - omitted `summary` remains `extractive`;
  - provided `summary` remains `provided`.

## Non-Goals

- Destructive truncation of old runtime turns or items.
- Network calls during tests.
- A new user-facing model configuration section.
- TUI hunk browsing or manual compaction confirmation.

## Verification

- `cargo test compact_thread_writes_summary_item_and_event --lib`
- `cargo test tool_fields_are_omitted_for_no_tool_requests --lib`
- `cargo test model_compaction_summary_request_captures_prior_context --lib`
- `cargo test runtime_daemon_compaction_uses_model_summary_provider --lib`
- `cargo test runtime_daemon_tick_compacts_threads_after_usage_warning --lib`
- `cargo fmt --check`
- `git diff --check`

## Remaining Differences

- TUI reasoning-content browsing and replay controls remain separate Phase E
  work.
- Model summaries are bounded transcript summaries; future work can include
  tool-output-aware summarization or provider-specific lower-cost summary
  routing.
