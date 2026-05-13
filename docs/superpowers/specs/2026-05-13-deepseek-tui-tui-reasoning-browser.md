# DeepSeek-TUI TUI Reasoning Browser

## Context

DeepSeekCode already persists streamed model reasoning as durable runtime
`reasoning` items, shows reasoning item counts in the task panel, and replays a
small number of recent reasoning snippets into TUI/daemon agent runs.

The remaining Phase E UX gap is that reasoning content is hard to inspect or
control from the TUI. Operators need a visible reasoning browser and a local
control for how much persisted reasoning is replayed into the next TUI-started
agent run.

## Scope

- Add TUI command-palette commands:
  - `reasoning` / `reasoning list` opens a right-side reasoning detail panel.
  - `reasoning latest` and `reasoning show <latest|last|index|item-id|turn-id>`
    show full reasoning item content.
  - `reasoning replay <0..20>` sets the number of latest persisted reasoning
    entries replayed into subsequent local TUI agent runs.
- Use the existing scrollable right-side detail panel and keybindings.
- Keep runtime storage unchanged: reasoning remains durable `ItemRecord`
  content with `item_type = "reasoning"`.

## Non-Goals

- Editing or deleting reasoning records.
- Changing daemon replay limits.
- Sending replay-limit controls to remote HTTP-runtime TUI mode.
- Exposing hidden chain-of-thought in places where a provider did not stream or
  persist reasoning content.

## Verification

- `cargo test command_palette_opens_reasoning_detail_and_sets_replay_limit --lib`
- `cargo test command_palette_shows_reasoning_item_by_selector --lib`
- `cargo test composer_submits_user_message_action_for_active_thread --lib`
- `cargo test render_mcp_detail_uses_right_side_panel --lib`
- `cargo fmt --check`
- `git diff --check`

## Follow-Up

- In-panel search/highlighting and per-turn replay pinning are covered by
  `2026-05-13-deepseek-tui-tui-reasoning-search-pinning.md`.
