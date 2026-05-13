# DeepSeek-TUI RLM Process Session Continuation

Date: 2026-05-13

Status: implemented

## Gap

`rlm_process` could persist bounded summaries under
`.dscode/rlm-model/<session_id>.json` and inject prior context into later
long-input calls. It still required every process call to provide a fresh
`file_path` or `content`, which made it less REPL-like than DeepSeek-TUI RLM
process workflows where a user can continue reasoning from the active process
context.

## Spec

- Keep `file_path` / `content` required for new RLM process sessions and
  stateless calls.
- When `task` and `session_id` are provided without `file_path` / `content`,
  allow the call only if the durable session exists and has at least one prior
  turn.
- Render the child-agent prompt with the prior RLM session context plus an
  explicit "session context only" input marker.
- Append the continuation as another bounded session turn after the child model
  call completes.
- Preserve `reset=true` semantics: a reset session has no prior turns, so it
  cannot be continued without new input.
- Update model tool schemas, MCP schemas, runtime docs, and parity docs to
  describe session-only continuation.

## Implementation

- Added `load_rlm_process_input_or_session_context` for `rlm_process` and its
  aliases.
- Added a safe `session context only` synthetic process input when the session
  has prior turns and no new source is provided.
- Kept `rlm_chunk_plan`, `rlm_map_reduce_plan`, and `rlm_recursive_plan`
  unchanged; those remain input-planning helpers and still require
  `file_path` or `content`.
- Updated DeepSeek static tool specs and MCP tool definitions.
- Updated runtime and gap docs.

## Verification

- `/home/willamhou/.cargo/bin/cargo test rlm_process_session_only_continuation_uses_prior_context --lib`
- `/home/willamhou/.cargo/bin/cargo test rlm_process --lib`
- `/home/willamhou/.cargo/bin/cargo test rlm_model_session --lib`
- `/home/willamhou/.cargo/bin/cargo test build_tool_specs_include_rlm --lib`
- `/home/willamhou/.cargo/bin/cargo test serve --lib`
- `/home/willamhou/.cargo/bin/cargo fmt --check`
- `/home/willamhou/.cargo/bin/cargo check`
- `git diff --check`

## Remaining Gap

This narrows model-backed RLM from durable context injection to actual
session-only continuation, but it is still not a continuously live model daemon.
Each continuation is a bounded child-agent call over persisted summaries rather
than a resumable model connection with independent lifecycle, cancellation, and
recovery after the owning DeepSeekCode process exits.
