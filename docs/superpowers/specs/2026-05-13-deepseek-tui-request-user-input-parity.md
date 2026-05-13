# DeepSeek-TUI Request User Input Parity Spec

日期：2026-05-13

对比对象：`Hmbown/DeepSeek-TUI`，`main` HEAD `3382242`

## 背景

DeepSeek-TUI exposes `request_user_input` so the model can ask the user 1-3
short structured questions with 2-3 options each. DeepSeekCode can already ask
plain-text clarifying questions through normal assistant output, but it does not
expose the DeepSeek-TUI-compatible tool name or validate the structured payload.

## 目标

- Add agent-visible `request_user_input`.
- Accept a required `questions` JSON array with 1-3 question objects.
- Validate `header`, `id`, `question`, and `options` fields.
- Require each question to include 2-3 options with non-empty `label` and
  `description`.
- Return a clear non-mutating prompt summary that tells the model to stop and
  ask the user for the requested selections.
- Expose the tool in the registry and DeepSeek model tool schema.

## 非目标

- This slice does not add a blocking TUI modal or suspend/resume the agent loop.
- This slice does not persist user-input requests into `.dscode/runtime`.
- This slice does not add HTTP endpoints for user-input decisions.

## 验收标准

1. `request_user_input` accepts a DeepSeek-TUI-style `questions` array.
2. Empty questions, more than 3 questions, missing required question fields, and
   options outside 2-3 items are rejected.
3. Valid requests return a stable `meta.user_input_required=true` summary with
   question ids and option labels.
4. The default registry exposes `request_user_input`.
5. The model schema exposes `request_user_input` with nested question/options
   properties.

## 实现结果

- Added `RequestUserInputTool` in `src/tools/user_input.rs`.
- `request_user_input` accepts a required `questions` JSON array carried through
  the existing nested-tool-argument serialization path.
- The tool validates:
  - `questions` must contain 1-3 objects.
  - each question must include non-empty `header`, `id`, and `question` strings.
  - each question must include an `options` array with 2-3 option objects.
  - each option must include non-empty `label` and `description` strings.
- Valid requests return `meta.user_input_required=true`, the question count,
  question ids, question text, and option labels/descriptions in a stable
  non-mutating summary.
- Registered `request_user_input` in the default agent tool registry.
- Added the DeepSeek model tool schema with nested question/options properties
  and `minItems` / `maxItems` bounds.
- Updated runtime docs and the DeepSeek-TUI parity plan. Blocking TUI modal
  handling remains a separate parity gap.

## 验证

- `cargo test request_user_input`: passed, 5 tests.
- `cargo test default_registry_includes_read_only_git_history_tools`: passed.
- `cargo fmt --check`: passed.
- `git diff --check`: passed.
- `cargo test`: passed, 971 tests.
- `cargo package --allow-dirty`: passed, packaged 285 files and verified
  `deepseek_code v0.1.0`.
