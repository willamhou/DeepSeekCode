# DeepSeek-TUI TUI Absolute Slash Paths

**Status:** implemented
**Comparison source:** `Hmbown/DeepSeek-TUI` refreshed at `/tmp/deepseek-tui-compare-20260514`; latest fetched `origin/main` `13e7957621448792beda06ec8615e33cb374adce`, including upstream commit `019d556` (`fix(tui): treat absolute slash paths as messages`).

## Gap

DeepSeek-TUI now treats absolute slash-prefixed filesystem paths such as
`/usr/bin` as composer message text instead of sending them through slash
command parsing. DeepSeekCode's local file-backed TUI had the same risk at the
custom slash fallback: `/usr/lib/...` could be interpreted as a missing custom
slash command instead of being submitted as a normal user request.

## Implementation

- Added slash-token helpers shared by custom slash parsing, completion, and
  composer hinting.
- Kept normal slash commands such as `/help`, `/model`, and `/memory path`
  unchanged.
- Treated slash tokens containing another `/` as path-like unless they are
  registered project or extra custom slash commands.
- Suppressed slash completion "no matches" hints for path-like absolute
  prefixes such as `/usr/lib`.
- Preserved registered nested custom slash commands such as `/pr/fix`.

## Verification

- `/home/willamhou/.cargo/bin/cargo fmt --check`
- `/home/willamhou/.cargo/bin/cargo test absolute_slash_path --lib`
- `/home/willamhou/.cargo/bin/cargo test slash_hints_ignore_absolute_path_prefixes --lib`
- `/home/willamhou/.cargo/bin/cargo test path_like_custom_slash_command --lib`
- `/home/willamhou/.cargo/bin/cargo test composer_slash --lib`
- `/home/willamhou/.cargo/bin/cargo test composer_ --lib`
- `/home/willamhou/.cargo/bin/cargo check`
- `/home/willamhou/.cargo/bin/cargo test --lib -- --test-threads=1`
- `git diff --check`

## Remaining

This closes the absolute slash path routing regression for the local TUI
composer. It does not change command-palette behavior, where entering command
text is already an explicit command-mode action.
