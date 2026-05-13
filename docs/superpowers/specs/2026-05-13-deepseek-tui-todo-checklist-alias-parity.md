# DeepSeek-TUI Todo Checklist Alias Parity Spec

日期：2026-05-13

对比对象：`Hmbown/DeepSeek-TUI`，`main` HEAD `3382242`

## 背景

DeepSeek-TUI exposes both `todo_*` compatibility tools and `checklist_*` tools:
write, add, update, and list. DeepSeekCode already had `todo_write`, but models
that call incremental todo/checklist tools could not update the in-memory plan
without replacing the whole list.

## 目标

- Add `todo_add`, `todo_update`, and `todo_list`.
- Add `checklist_write`, `checklist_add`, `checklist_update`, and
  `checklist_list` aliases.
- Share the same in-memory `TodoList` used by `todo_write`.
- Use 1-based ids for incremental update because DeepSeekCode's current todo
  records do not have durable ids.
- Expose all new names in OpenAI/Anthropic tool schemas.

## 非目标

- This slice does not add durable checklist persistence beyond existing session
  save/load behavior.
- This slice does not add per-item UUIDs.
- This slice does not alter the offline planner's preference for `todo_write`.

## 验收标准

1. Default registry exposes all todo/checklist tool names.
2. `todo_add` appends an item to the shared list.
3. `todo_update` updates a 1-based item id.
4. `todo_list` renders the shared list.
5. `checklist_write` reuses `todo_write` validation.
6. OpenAI/Anthropic schemas include all new aliases.

## 实现结果

- `src/tools/todo.rs` adds incremental add/update/list tools and checklist
  aliases.
- `src/tools/registry.rs` registers all names against the shared `TodoList`.
- `src/model/deepseek.rs` exposes schemas for all names.
- `docs/runtime.md` and the DeepSeek-TUI parity plan document the compatibility
  surface.

## 验证

- `/home/willamhou/.cargo/bin/cargo fmt --check`
- `/home/willamhou/.cargo/bin/cargo test todo_`
- `/home/willamhou/.cargo/bin/cargo test checklist`
- `/home/willamhou/.cargo/bin/cargo test`
- `git diff --check`
- `/home/willamhou/.cargo/bin/cargo package --allow-dirty`
