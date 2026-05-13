# DeepSeek-TUI Review Semantic Delegation Parity

## Gap

DeepSeekCode has an agent-visible `review` tool with deterministic marker and
behavioral signals, but DeepSeek-TUI's review flow is closer to a semantic code
review assistant that reasons over behavior, regressions, and missing tests.
The deterministic pass is useful evidence, but it cannot catch broader logic
risks by itself.

## Target

- Keep deterministic `review` as the default fast local path.
- Add opt-in `semantic=true` to run a read-only child-agent semantic review over
  the same file or diff.
- Feed the child reviewer both the source/diff and deterministic baseline JSON.
- Return the semantic child summary inside the structured review JSON.
- Prevent recursive semantic review at the subagent depth limit.

## Acceptance Criteria

1. Existing `review target=<file|diff>` behavior stays unchanged by default.
2. The model schema exposes `semantic`, `steps`, `agent`, and `skill`.
3. `semantic=true` requires a configured registry-backed `ReviewTool` and returns
   a clear error when invoked from an unconfigured unit test tool.
4. The semantic task prompt includes deterministic review JSON, source under
   review, and a read-only instruction.
5. Runtime docs and the parity plan describe the semantic option.
6. Focused tests and full Rust gates pass.

## Implementation Notes

- `ReviewTool::new(config, parent_depth)` is used by the default registry.
- Unit tests use `ReviewTool::default()` for deterministic-only behavior.
- Semantic mode delegates through `dispatch_subagent` with the requested
  `steps`, `agent`, and `skill` inputs.

## Verification

- `/home/willamhou/.cargo/bin/cargo test review_semantic`
- `/home/willamhou/.cargo/bin/cargo test review`
- `/home/willamhou/.cargo/bin/cargo test build_tool_specs_include_review`
- `/home/willamhou/.cargo/bin/cargo test`
- `/home/willamhou/.cargo/bin/cargo fmt --check`
- `git diff --check`
- `/home/willamhou/.cargo/bin/cargo package --allow-dirty`
