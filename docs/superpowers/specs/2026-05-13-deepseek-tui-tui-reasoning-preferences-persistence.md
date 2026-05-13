# DeepSeek-TUI TUI Reasoning Preferences Persistence

Date: 2026-05-13

Status: completed

## Gap

Reasoning replay search and pinning gave operators control over which durable
reasoning entries local TUI agent runs replayed, but those preferences were only
in-memory TUI state. Restarting the file-backed TUI reset the replay limit and
pin set.

## Spec

1. Persist local file-backed TUI replay preferences at
   `.dscode/tui/reasoning-replay.json`.
2. Store a stable JSON envelope with:
   - `kind = "deepseek.tui.reasoning_replay.v1"`
   - `replay_limit`
   - `pinned_turn_ids`
3. Clamp loaded replay limits to the existing `0..20` command range.
4. Ignore missing preference files, warn in the TUI status on corrupt files, and
   keep runtime thread/item storage unchanged.

## Implementation

- `deepseek tui` enables preference persistence after loading the local runtime
  store.
- `TuiApp` loads preferences once when enabled and writes them after successful
  `reasoning replay`, `reasoning pin`, `reasoning unpin`, and `reasoning unpin
  all` commands.
- The preference file only stores local replay controls; the replay content
  still comes from durable runtime `reasoning` items.

## Verification

- `/home/willamhou/.cargo/bin/cargo test reasoning_replay_preferences_persist_across_tui_instances --lib`
- `/home/willamhou/.cargo/bin/cargo test reasoning_replay --lib`
