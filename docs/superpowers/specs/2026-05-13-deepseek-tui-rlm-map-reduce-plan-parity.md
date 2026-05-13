# DeepSeek-TUI RLM Map-Reduce Plan Parity

## Gap

`rlm_chunk_plan` gives DeepSeekCode a direct chunk planner, but the model still
has to manually turn chunks into map tasks and a reduce prompt. DeepSeek-TUI-style
RLM workflows usually make that orchestration explicit: chunk, map, then reduce
with coverage and omitted-section awareness.

## Target

- Add read-only `rlm_map_reduce_plan`.
- Accept `task` (or `question`) plus workspace-relative `file_path` or inline
  `content`.
- Reuse chunk sizing, overlap, `include_text`, and long-input safety rules from
  `rlm_chunk_plan`.
- Return chunks, coverage metadata, ready-to-dispatch map task JSON, omitted map
  count when `map_limit` is smaller than chunk count, and a reduce prompt.
- Do not run child agents directly; this tool only plans the workflow.

## Acceptance Criteria

1. `rlm_map_reduce_plan` requires a non-empty `task` or `question`.
2. It emits map tasks that include chunk index, offsets, suggested `steps`, and
   a self-contained map prompt when `include_text=true`.
3. `include_text=false` hides chunk text and makes map prompts offset-based.
4. `map_limit` is clamped to the RLM batch limit and reports omitted tasks.
5. Registry exposes the tool below the RLM depth limit and hides it at the
   limit.
6. OpenAI/Anthropic tool schema includes `rlm_map_reduce_plan`.
7. Runtime docs and the parity plan mention the helper.
8. Focused tests and full Rust gates pass.

## Verification

- `/home/willamhou/.cargo/bin/cargo test rlm_map_reduce_plan`
- `/home/willamhou/.cargo/bin/cargo test default_registry_includes_dispatch_subagent_only_below_max_depth`
- `/home/willamhou/.cargo/bin/cargo test build_tool_specs_include_rlm`
- `/home/willamhou/.cargo/bin/cargo test`
- `/home/willamhou/.cargo/bin/cargo fmt --check`
- `git diff --check`
- `/home/willamhou/.cargo/bin/cargo package --allow-dirty`
