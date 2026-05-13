# DeepSeek-TUI Persisted Reasoning Replay

## Context

DeepSeekCode already captured streaming reasoning deltas as runtime `reasoning`
items for TUI-started agent runs, and the in-process agent loop replayed recent
reasoning into later requests during the same run. The gap was cross-run
continuity: a later TUI or daemon task on the same durable thread did not preload
persisted reasoning items, so model requests could lose prior thinking context
after a tool loop, cancellation, or separate queued task.

## Scope

- Seed `AgentLoop` recent-step replay from caller-provided persisted entries.
- Add a runtime helper that extracts the latest durable `reasoning` items for a
  thread in oldest-to-newest order.
- Preload those entries for runtime daemon task execution and TUI-started agent
  runs.
- Keep entries compact and bounded to avoid prompt bloat.

## Implementation

- `AgentLoopOptions.initial_recent_steps` seeds the same replay window used for
  assistant/reasoning continuity inside one run.
- `RuntimeStore::recent_reasoning_replay_entries(thread_id, limit)` reads
  persisted runtime reasoning items, compacts their content, and includes the
  source turn id when present.
- `deepseek agents run-task`, the local daemon task runner, and local TUI agent
  runs pass the latest three persisted reasoning entries into `AgentLoop`.
- Runtime docs and the DeepSeek-TUI parity plan now describe cross-run
  persisted reasoning replay.

## Verification

- `cargo test recent_reasoning_replay_entries_reads_persisted_reasoning_items --lib`
- `cargo test run_with_client_replays_initial_recent_steps_on_first_request --lib`
- `cargo test run_with_client_replays_recent_reasoning_into_next_request --lib`
- `cargo fmt --check`
- `git diff --check`

## Remaining

This slice preloads compact persisted reasoning entries into model requests. A
richer TUI browsing surface and user-tunable replay controls remain future work.
