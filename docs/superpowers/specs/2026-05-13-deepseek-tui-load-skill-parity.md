# DeepSeek-TUI Load Skill Parity Spec

日期：2026-05-13

对比对象：`Hmbown/DeepSeek-TUI`，`main` HEAD `3382242`

## 背景

DeepSeek-TUI exposes `load_skill` so the model can fetch a skill body and
companion-file list by skill name instead of spending separate `read_file` and
`list_dir` calls.

DeepSeekCode already has TOML-backed repo/user skills and skill auto-select,
but the agent tool surface does not expose a direct `load_skill` tool.

## 目标

- Add read-only `load_skill`.
- Use the existing DeepSeekCode TOML skill registry shape instead of adding a
  second SKILL.md format.
- Search repo skills and configured user skills with the same last-wins
  override semantics.
- Return a self-contained skill context: description, source path,
  allowed tools, triggers, suggested steps, initial todos, references, policy,
  and `system_append`.
- Report available skill names when lookup fails.
- Expose the tool through model schemas and the default registry.

## 非目标

- This slice does not convert DeepSeekCode skills from TOML to SKILL.md.
- This slice does not implement remote skill installation.
- This slice does not mutate skill files.

## 验收标准

1. `load_skill name=<skill>` returns the selected skill context.
2. User skills override repo skills when names collide.
3. Missing skills return a useful available-skill hint.
4. Registry and model schemas include `load_skill`.
5. The tool is read-only and does not require approval.

## 实现结果

- Added `src/tools/skill.rs` with read-only `load_skill`.
- The tool searches the install-portable repo skill directory and configured
  `workspace.user_skills_dir`, with user skills overriding repo skills.
- The result renders description, source path, allowed tools, triggers,
  suggested steps, initial todos, references, policy, and `system_append`.
- Missing skills report available names and searched directories.
- Registered `load_skill` in the default registry and model schemas.
- Documented the TOML-skill mapping in runtime and parity docs.

## 验证

- `cargo test load_skill` passed: 5 tests.
- `cargo test default_registry_includes_read_only_git_history_tools` passed.
- `cargo fmt --check` passed.
- `git diff --check` passed.
- `cargo test` passed: 940 tests.
- `cargo package --allow-dirty` passed: 275 packaged files.
