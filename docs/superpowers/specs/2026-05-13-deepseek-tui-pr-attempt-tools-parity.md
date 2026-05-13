# DeepSeek-TUI PR Attempt Tools Parity Spec

日期：2026-05-13

对比对象：`Hmbown/DeepSeek-TUI`，`main` HEAD `3382242`

## 背景

DeepSeek-TUI exposes PR-attempt evidence tools:
`pr_attempt_record`, `pr_attempt_list`, `pr_attempt_read`, and
`pr_attempt_preflight`. They let an agent capture a current git diff as a
durable PR work attempt, list/read attempts, and run a non-mutating
`git apply --check` preflight against a recorded patch.

DeepSeekCode already has runtime tasks and guarded GitHub tools, but it does not
yet expose this PR-attempt evidence surface.

## 目标

- Add agent-visible `pr_attempt_record`, `pr_attempt_list`, `pr_attempt_read`,
  and `pr_attempt_preflight`.
- Store attempt metadata and patch artifacts under `.dscode/runtime/pr_attempts`.
- `pr_attempt_record` captures `git diff --binary --no-color`, changed files,
  current branch/SHA metadata, summary, verification notes, and optional
  `task_id`.
- `pr_attempt_list` lists recent attempts, optionally filtered by `task_id`.
- `pr_attempt_read` reads one attempt by `attempt_id` or `id`.
- `pr_attempt_preflight` runs `git apply --check <recorded.patch>` in the
  recorded workspace and reports stdout/stderr summaries without mutating files.
- Expose all four tools in the default registry and DeepSeek model schema.

## 非目标

- This slice does not change the durable `TaskRecord` JSON format.
- This slice does not apply or select an attempt.
- This slice does not post PRs or mutate GitHub state.

## 验收标准

1. `pr_attempt_record summary=<text>` rejects an empty working-tree diff.
2. Valid record calls write metadata and a patch artifact.
3. List/read calls can retrieve recorded attempts by id and task id.
4. Preflight reports `would_apply`, `exit_code`, and stdout/stderr summaries
   while keeping `mutated_worktree=false`.
5. The default registry exposes all four tool names.
6. The model schema exposes all four tool names and their key parameters.

## 实现结果

- Added `pr_attempt_record`, `pr_attempt_list`, `pr_attempt_read`, and
  `pr_attempt_preflight` in `src/tools/runtime_tasks.rs`.
- Attempt metadata is stored as JSON under `.dscode/runtime/pr_attempts`, and
  each recorded attempt writes a sibling `.patch` artifact.
- `pr_attempt_record` captures `git diff --binary --no-color`, changed files,
  branch/SHA metadata, `summary`, optional `task_id`, attempt group/index/count,
  and verification notes.
- `pr_attempt_list` returns recent attempts with optional `task_id` filtering.
- `pr_attempt_read` reads by `attempt_id` or `id`, with optional `task_id`
  assertion.
- `pr_attempt_preflight` loads the recorded patch, verifies it is inside the
  attempt store, and runs `git apply --check` in the recorded workspace. The
  result includes `would_apply`, `exit_code`, stdout/stderr summaries, and
  `mutated_worktree=false`.
- Registered all four tools in the default agent registry.
- Added all four DeepSeek model schema entries.
- Updated runtime docs and the DeepSeek-TUI parity plan.

## 验证

- `cargo test pr_attempt`: passed, 2 tests.
- `cargo test build_tool_specs_include_runtime_task_and_automation_tools`:
  passed.
- `cargo test default_registry_includes_read_only_git_history_tools`: passed.
- `cargo fmt --check`: passed.
- `git diff --check`: passed.
- `cargo test`: passed, 973 tests.
- `cargo package --allow-dirty`: passed, packaged 286 files and verified
  `deepseek_code v0.1.0`.
