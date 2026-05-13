# DeepSeek-TUI Request User Input Other Option Parity Spec

日期：2026-05-13

对比对象：`Hmbown/DeepSeek-TUI`，`main` HEAD `3382242`

## 背景

DeepSeekCode now supports durable and blocking runtime-backed
`request_user_input`, but the TUI modal only accepts numbered predefined
options. The tool protocol expects clients to offer an "Other" path for
short free-form answers when none of the predefined choices fit.

## 目标

- Add an "Other" entry mode to the TUI user-input modal.
- Let `o` enter free-form answer mode for the current question.
- Let typed characters, Backspace, and Enter edit/submit the answer.
- Preserve multi-question flow: submitting Other advances to the next question
  or emits the final `RespondUserInput` action.
- Keep Escape behavior predictable: leave Other edit mode first, dismiss the
  modal when not editing Other.

## 非目标

- This slice does not change the model tool schema.
- This slice does not add multi-line free-form text.
- This slice does not change HTTP event payload shapes.

## 验收标准

1. User-input modal renders an Other affordance.
2. `o` starts free-form answer editing for the active question.
3. Character keys and Backspace update the draft answer.
4. Enter submits the draft answer through `TuiAction::RespondUserInput`.
5. Empty Other submissions are rejected without closing the modal.
6. Existing numbered option flow still works.

## 实现结果

- Added TUI user-input Other mode with a bounded single-line draft answer.
- `o` starts Other editing, typed characters append to the draft, Backspace
  removes the last character, Enter submits, and Esc exits Other editing.
- Empty Other submissions keep the modal open and report a status message.
- Numbered option selection and Other submission now share the same answer
  finalization path for multi-question flow and final `RespondUserInput`.
- Runtime, TUI, and parity-plan docs describe Other answers.

## 验证

- `cargo test user_input_modal_accepts_other_answer`: passed.
- `cargo test replace_runtime_with_user_input_opens_modal_and_records_answer`:
  passed.
- `cargo test user_input`: passed, 14 tests.
- `cargo test`: passed, 1028 tests.
- `cargo fmt --check`: passed.
- `git diff --check`: passed.
- `cargo package --allow-dirty`: passed, packaged 305 files.
