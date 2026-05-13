# DeepSeek-TUI Recall Archive Parity

Date: 2026-05-13

## Gap

DeepSeek-TUI exposes `recall_archive` so the agent can search older cycle
archives when compacted context is missing from the current prompt. DeepSeekCode
already has durable runtime threads, turns, items, and compaction summaries, but
had no agent-visible tool with the DeepSeek-TUI `recall_archive` name.

## Scope

- Add agent-visible `recall_archive`.
- Search durable `.dscode/runtime` thread records instead of DeepSeek-TUI's
  numbered cycle JSONL archives.
- Search turns, items, and compaction summary items.
- Accept `query`, optional `thread_id`, and `max_results`/`limit`.
- Expose the DeepSeek-TUI `cycle` argument in the schema for compatibility,
  while documenting that local recall uses runtime threads.
- Keep the tool read-only and approval-free.
- Expose OpenAI-compatible and Anthropic-compatible schemas.

## Acceptance

- `recall_archive` is present in the default registry.
- Model schemas include `recall_archive` with `query` and `max_results`.
- A query searches persisted runtime turn/item content and returns ranked hits
  with thread ids, source type, score, and excerpt.
- Empty or tokenless queries are rejected.
- Runtime docs and the DeepSeek-TUI parity plan mention `recall_archive`.

## Implementation

- Added `src/tools/recall_archive.rs`.
- Registered the module in `src/tools/mod.rs`.
- Added `RecallArchiveTool` to the default registry.
- Added a static model schema in `src/model/deepseek.rs`.
- Updated runtime docs and the DeepSeek-TUI parity plan.

## Verification

- `cargo test recall_archive --lib` passed: 2 tests.
- `cargo test build_tool_specs_include_recall_archive --lib` passed.
- `cargo test default_registry_includes_todo_checklist_compat_tools --lib`
  passed.
- `cargo fmt --check` passed.
- `git diff --check` passed.
- `cargo test` passed: 1086 library tests plus bin/doc-test targets.
- `cargo package --allow-dirty` passed.
