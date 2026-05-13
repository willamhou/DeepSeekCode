# DeepSeek-TUI TUI Reasoning Search and Pinning

Date: 2026-05-13

Status: completed

## Gap

The TUI reasoning browser could list and open persisted reasoning items, and it
could replay the latest N reasoning entries into later local TUI agent runs. The
remaining Phase E gap was finer operator control: search/highlight reasoning
content in the panel, and pin important reasoning turns so they stay in replay
even after they fall outside the latest-N window.

## Spec

1. Add command-palette search:
   - `reasoning search <query>` opens the reasoning panel with matching items.
   - Matches cover item id, turn id, status, and content.
   - Matching excerpts mark query hits inline with `[[...]]`.
2. Add per-turn replay pinning:
   - `reasoning pin <latest|index|item-id|turn-id>` pins the selected item's
     `turn_id` for local TUI replay.
   - `reasoning pins` shows the pinned turn set and matching reasoning item
     counts.
   - `reasoning unpin <selector|all>` removes one pin or clears all pins.
3. Preserve the existing replay limit behavior: pinned turns are additive to
   `reasoning replay <0..20>` and deduplicated with the latest-N items.
4. Keep runtime storage unchanged. Pins are local TUI state, while replay entry
   construction reads existing durable `reasoning` items.

## Verification

- `/home/willamhou/.cargo/bin/cargo test command_palette_searches_reasoning_with_highlight --lib`
- `/home/willamhou/.cargo/bin/cargo test command_palette_pins_reasoning_turns_for_replay --lib`
- `/home/willamhou/.cargo/bin/cargo test reasoning_replay_entries_include_pinned_turns_beyond_latest_limit --lib`
- `/home/willamhou/.cargo/bin/cargo test command_palette_opens_reasoning_detail_and_sets_replay_limit --lib`
- `/home/willamhou/.cargo/bin/cargo test command_palette_shows_reasoning_item_by_selector --lib`
- `/home/willamhou/.cargo/bin/cargo test tui::tests --lib`
- `/home/willamhou/.cargo/bin/cargo test core::runtime::tests::recent_reasoning_replay_entries_reads_persisted_reasoning_items --lib`
- `/home/willamhou/.cargo/bin/cargo fmt --check`
- `git diff --check`

## Implementation

- `TuiApp` now tracks local pinned reasoning turn ids and exposes them to the TUI
  action handler when launching a local agent run.
- The reasoning panel renders search result details, highlighted excerpts, and
  replay pin state in the same scrollable right-side panel as the existing
  browser.
- `RuntimeStore::reasoning_replay_entries_with_pinned_turns` merges latest-N
  reasoning items with pinned-turn reasoning items, deduplicates by item id, and
  returns oldest-first replay entries.
