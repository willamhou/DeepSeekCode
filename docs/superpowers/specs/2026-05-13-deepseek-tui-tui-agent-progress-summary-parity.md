# DeepSeek-TUI TUI Agent Progress Summary Parity

Date: 2026-05-13

## Gap

DeepSeekCode's task panel showed active-thread task records and a raw runtime
item count, but it did not summarize streamed item progress. During background
TUI agent runs, users could see that items existed but had to inspect the
transcript to understand whether the run was producing messages, tool calls, or
tool results.

## Scope

- Keep durable task record rendering unchanged.
- Summarize active-thread runtime item states in the task panel.
- Summarize active-thread runtime item types in the task panel.
- Show the latest non-empty runtime item as a compact progress line.
- Reuse existing active-thread items already loaded into the TUI; do not change
  runtime storage or AgentLoop event contracts.
- Document the progress summary in `docs/tui.md` and the parity plan.

## Acceptance

- The task panel still shows `Runtime items: N`.
- When active-thread items exist, the panel includes item state counts such as
  `running=1 completed=1`.
- The panel includes item type counts such as `message=1 tool_call=1`.
- The panel includes the latest item type/status/content summary.
- Existing task panel and TUI tests remain green.

## Verification

- `cargo test task_panel_renders_active_thread_runtime_tasks --lib`: passed.
- `cargo test tui --lib`: 105 passed.
- `cargo fmt --check`: passed.
- `git diff --check`: passed.
- `cargo test`: 1072 passed.
- `cargo package --allow-dirty`: passed.
