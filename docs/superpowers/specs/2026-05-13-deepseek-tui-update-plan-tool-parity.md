# DeepSeek-TUI Update Plan Tool Parity

Date: 2026-05-13

## Gap

DeepSeek-TUI exposes an agent-visible `update_plan` tool that accepts a
structured implementation plan with step statuses. DeepSeekCode had
`todo_write`, `todo_add`, and DeepSeek-TUI-compatible checklist aliases, but not
the canonical `update_plan` name or its `{explanation, plan:[{step,status}]}`
input shape.

## Scope

- Add agent-visible `update_plan`.
- Accept optional `explanation`.
- Accept `plan` as a JSON array of `{step, status}` objects.
- Support `pending`, `in_progress`, and `completed`, with tolerant aliases
  `inprogress` and `done`.
- Reuse the existing in-memory todo/checklist state instead of introducing a
  separate planning store.
- Preserve the DeepSeek-TUI single-`in_progress` invariant by demoting duplicate
  `in_progress` steps to `pending`.
- Expose the tool schema to OpenAI-compatible and Anthropic-compatible tool
  calling.

## Acceptance

- `update_plan` is present in the default registry.
- OpenAI and Anthropic tool schemas include `update_plan` with `plan`.
- A valid update writes the current plan into the shared todo list and returns a
  progress summary.
- Duplicate `in_progress` steps leave only the first step in progress.
- Invalid plan shapes are rejected with a clear error.
- Documentation and the DeepSeek-TUI parity plan mention `update_plan`.

## Implementation

- Added `UpdatePlanTool` in `src/tools/todo.rs`.
- Registered it in `src/tools/registry.rs` on the same `TodoList` backing state
  as `todo_write` and `checklist_write`.
- Added a static model schema in `src/model/deepseek.rs`.
- Updated runtime docs and the DeepSeek-TUI parity plan.

## Verification

- `cargo test update_plan --lib` passed: 3 tests.
- `cargo test default_registry_includes_todo_checklist_compat_tools --lib`
  passed.
- `cargo test todo_checklist_tools --lib` passed: 2 tests.
- `cargo fmt --check` passed.
- `git diff --check` passed.
- `cargo test` passed: 1080 tests.
- `cargo package --allow-dirty` passed.
