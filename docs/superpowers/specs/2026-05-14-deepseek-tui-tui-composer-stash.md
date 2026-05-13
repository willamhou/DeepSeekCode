# DeepSeek-TUI TUI Composer Stash

Status: implemented

## Gap

DeepSeek-TUI supports parking an in-progress composer draft with `Ctrl+S` and
restoring it later through `/stash pop`. Before this slice, DeepSeekCode's TUI
composer could edit and submit text, but it had no draft stash workflow for
temporarily clearing the composer without losing the draft.

## Implementation

- Added a local composer stash model with file-backed persistence at
  `.dscode/tui/composer-stash.json` for normal local TUI sessions and in-memory
  behavior for demo/tests without a configured path.
- Added `Ctrl+S` handling while the composer is focused; non-empty drafts are
  parked, bounded to the latest 100 entries, persisted, and the composer is
  cleared.
- Added command-palette and slash-style routes for `stash list`, `stash pop`,
  `stash clear`, plus `/stash list`, `/stash pop`, `/stash clear`, and the
  `/park` alias.
- Rendered stash listings in the right-side detail panel and restored the most
  recent draft into the focused composer on pop.
- Updated TUI documentation and the DeepSeek-TUI parity plan.

## Verification

- `cargo test stashes --lib`
- `cargo test composer_stash --lib`
- `cargo test tui --lib`
- `cargo fmt --check`
- `cargo check`
- `git diff --check`

## Remaining

The stash stores timestamps as the existing runtime-style `epoch+seconds`
labels. Human-readable wall-clock formatting can be added later if the TUI gets
a broader timestamp formatting utility.
