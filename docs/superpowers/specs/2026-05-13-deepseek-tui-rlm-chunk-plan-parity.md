# DeepSeek-TUI RLM Chunk Plan Parity

## Gap

DeepSeekCode can chunk context inside `rlm_python` via `chunk_context()`, but a
model must write Python before it can inspect coverage or prepare a map-reduce
plan. DeepSeek-TUI-style RLM workflows benefit from a direct helper that chunks
long input, reports coverage, and can feed follow-up RLM calls.

## Target

- Add read-only `rlm_chunk_plan`.
- Accept the same long-input shape as `rlm_process`: workspace-relative
  `file_path` or inline `content`.
- Return chunk `index`, `start`, `end`, `chars`, and optional `text`.
- Return coverage metadata: chunk count, context chars, covered chars, gaps, and
  completeness.
- Support `max_chars`, `overlap`, and `include_text=false`.
- Reuse the existing RLM process input safety checks and subagent depth gate.

## Acceptance Criteria

1. `rlm_chunk_plan content=...` splits inline content with overlap.
2. `include_text=false` returns offsets and `text:null`.
3. Invalid overlap is rejected.
4. Registry exposes `rlm_chunk_plan` below the RLM depth limit and hides it at
   the limit.
5. OpenAI/Anthropic tool schema includes `rlm_chunk_plan`.
6. Runtime docs and the parity plan mention the helper.
7. Focused tests and full Rust gates pass.

## Verification

- `/home/willamhou/.cargo/bin/cargo test rlm_chunk_plan`
- `/home/willamhou/.cargo/bin/cargo test default_registry_includes_dispatch_subagent_only_below_max_depth`
- `/home/willamhou/.cargo/bin/cargo test build_tool_specs_include_rlm`
- `/home/willamhou/.cargo/bin/cargo test`
- `/home/willamhou/.cargo/bin/cargo fmt --check`
- `git diff --check`
- `/home/willamhou/.cargo/bin/cargo package --allow-dirty`
