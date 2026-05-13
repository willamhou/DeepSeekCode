# DeepSeek-TUI TUI Memory Command Parity

Date: 2026-05-13

## Gap

DeepSeekCode already had opt-in user memory prompt injection and an
agent-visible `remember` tool, but the TUI did not expose DeepSeek-TUI's
interactive memory workflow. DeepSeek-TUI lets users add durable memory from the
composer with a single `#` prefix and inspect/manage memory through `/memory`.
DeepSeekCode required the model tool path or external file edits.

## Scope

- Intercept single-`#` composer lines as local memory writes without submitting a
  user turn.
- Preserve Markdown heading / shell-style inputs that start with `##` or `#!` as
  normal composer submissions.
- Add composer `/memory show|path|clear|edit|help` commands.
- Add command-palette `memory show|path|clear|edit|help` aliases.
- Keep all memory writes local and opt-in through existing `memory.enabled` /
  `DSCODE_MEMORY=on` config.
- Make HTTP-runtime TUI report memory commands as local file-backed only.

## Acceptance

- `# prefer cargo fmt` queues a memory append action and does not require an
  active thread.
- `/memory path` queues a memory path action and does not submit a user turn.
- `## markdown heading` still submits as a normal user message when a durable
  thread is active.
- Local action handling appends to `memory.memory_path`, can render/show memory,
  and can clear the file when enabled.
- Documentation and the DeepSeek-TUI parity plan mention the new TUI memory
  controls.

## Implementation

- Added `TuiAction::AppendMemory` and `TuiAction::Memory` with
  `TuiMemoryCommand`.
- Added composer intercepts for single-`#` notes and `/memory` commands.
- Added command-palette `memory` commands and completions.
- Added local TUI action handlers for append/show/path/clear/edit/help using the
  existing `append_user_memory` helper and configured `memory.memory_path`.
- Added a memory detail panel kind and HTTP-runtime local-only status handling.
- Updated `docs/tui.md`, `docs/runtime.md`, and the DeepSeek-TUI parity plan.

## Verification

- `cargo test command_palette_requests_memory_actions --lib` passed.
- `cargo test composer_intercepts_memory_prefix_and_slash_commands --lib`
  passed.
- `cargo test handle_tui_action_manages_memory_file --lib` passed.
- `cargo test tui --lib` passed: 110 tests.
- `cargo fmt --check` passed.
- `git diff --check` passed.
- `cargo test` passed: 1077 tests.
- `cargo package --allow-dirty` passed.
