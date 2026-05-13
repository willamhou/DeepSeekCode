# DeepSeek-TUI Git Diff Argument Parity Spec

日期：2026-05-13

对比对象：`Hmbown/DeepSeek-TUI`，`main` HEAD `3382242`

## 背景

DeepSeekCode already had `git_diff`, but it only ran a fixed `git diff -- .`.
DeepSeek-TUI's `git_diff` can scope by path, show staged changes, tune context
lines, and cap output.

## 目标

- Add `path`, `cached`, `unified`, and `max_chars` inputs to `git_diff`.
- Keep the existing no-argument behavior as "show working-tree diff".
- Use `--no-color` and `--no-ext-diff` for stable model-readable output.
- Expose the richer inputs through model and MCP schemas.

## 非目标

- This slice does not add named commit/range diffing.
- This slice does not add large-output retrieval.
- This slice does not change git history tools.

## 验收标准

1. `git_diff` without arguments still works.
2. `git_diff` supports staged diff through `cached=true`.
3. `git_diff` supports path scoping.
4. `git_diff` clamps `unified` to 0-50 and output to a bounded character count.
5. OpenAI/Anthropic/MCP schemas expose the new arguments.

## 实现结果

- `src/tools/git_diff.rs` now builds stable git diff arguments from input.
- `src/model/deepseek.rs` and `src/cli/commands/serve.rs` expose the richer
  schema.
- `docs/runtime.md` and the DeepSeek-TUI parity plan document the new behavior.

## 验证

- `cargo fmt --check`
- `cargo test git_diff`
- `cargo test builds_openai_tool_specs_for_known_tools`
- `cargo test`
- `git diff --check`
- `cargo package --allow-dirty`
