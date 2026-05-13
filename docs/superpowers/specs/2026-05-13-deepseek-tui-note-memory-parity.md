# DeepSeek-TUI Note/Memory Parity Spec

日期：2026-05-13

对比对象：`Hmbown/DeepSeek-TUI`，`main` HEAD `3382242`

## 背景

DeepSeek-TUI exposes two related persistence surfaces:

- `note`: append maintainer/agent notes to the configured notes file.
- `remember`: when user memory is enabled, append a durable single-sentence
  memory note and load that memory back into future system prompts.

DeepSeekCode currently has workspace instructions and in-memory todo state, but
no agent-visible persistent note or user-memory tool.

## 目标

- Add agent-visible `note` with required `content`.
- Add memory config with opt-in `memory.enabled`, `memory.memory_path`, and
  `memory.notes_path`.
- Add agent-visible `remember` only when memory is enabled.
- Append `note` entries to the configured notes file.
- Append `remember` entries as timestamped Markdown bullets and load the memory
  file into future system prompts.
- Keep both tools scoped to configured note/memory files and approval-free, like
  DeepSeek-TUI.
- Expose matching model schemas and registry entries.

## 非目标

- This slice does not add TUI `/note` management commands.
- This slice does not add memory import/include syntax.
- This slice does not add UI affordances for editing memory.

## 验收标准

1. `note content=<text>` appends to the configured notes file and rejects empty
   content.
2. `remember note=<text>` is registered only when memory is enabled.
3. `remember` rejects empty notes, strips leading `#`, and appends a Markdown
   bullet to the configured memory file.
4. Enabled memory content is loaded into the system prompt; disabled or missing
   memory is omitted.
5. Registry and model schemas include `note`, and include `remember` only when
   the enabled registry exposes it.
6. Config defaults are documented and parseable.

## 实现结果

- Added `src/tools/notes.rs` with `note` and `remember`.
- `note` accepts required `content` plus a `note` alias, rejects empty content,
  creates parent directories, and appends `---`-separated entries to
  `memory.notes_path`.
- Added `memory.enabled`, `memory.notes_path`, and `memory.memory_path` config,
  with `DSCODE_MEMORY` / `DEEPSEEK_MEMORY`,
  `DSCODE_NOTES_PATH` / `DEEPSEEK_NOTES_PATH`, and
  `DSCODE_MEMORY_PATH` / `DEEPSEEK_MEMORY_PATH` environment overrides.
- `remember` is registered only when `memory.enabled = true` or the memory env
  switch is on. It accepts required `note` plus a `content` alias, strips a
  leading `#`, rejects empty notes, and appends timestamped Markdown bullets to
  `memory.memory_path`.
- Enabled memory files are loaded into the agent system prompt as durable user
  memory; disabled, missing, or empty memory files are omitted.
- Registered `note` and conditionally registered `remember` in the default
  runtime registry.
- Added static model schemas for `note` and `remember`.
- Documented the tools and config in `docs/runtime.md` and the parity plan.
- Follow-up MCP work exposes `note` and enabled `remember` only in durable
  write-approval mode because they append persistent files.

## 验证

- `/home/willamhou/.cargo/bin/cargo test notes` passed: 3 tests.
- `/home/willamhou/.cargo/bin/cargo test memory` passed: 7 matching tests.
- `/home/willamhou/.cargo/bin/cargo test default_registry_includes_remember_only_when_memory_enabled` passed.
- `/home/willamhou/.cargo/bin/cargo test build_tool_specs_include_note_and_remember` passed.
- `/home/willamhou/.cargo/bin/cargo test default_registry_includes_read_only_git_history_tools` passed.
- `/home/willamhou/.cargo/bin/cargo test parse_config_overrides_memory_from_toml` passed.
- `/home/willamhou/.cargo/bin/cargo test build_system_prompt_includes_user_memory` passed.
- `/home/willamhou/.cargo/bin/cargo fmt --check` passed.
- `git diff --check` passed.
- `/home/willamhou/.cargo/bin/cargo test` passed: 956 tests.
- `/home/willamhou/.cargo/bin/cargo package --allow-dirty` passed: packaged
  279 files and verified `deepseek_code v0.1.0`.
