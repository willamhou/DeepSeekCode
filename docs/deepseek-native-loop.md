# DeepSeek-Native Agent Loop Design

Last updated: 2026-05-24

This document records the DeepSeek-specific agent-loop ideas worth absorbing
from external projects and turns them into a DeepSeekCode design plan.

## References

- [DeepSeek-Reasonix](https://github.com/esengine/DeepSeek-Reasonix), reviewed
  at commit `4610d54743732312261fdfaca8ae48bc32d876b0`.
- [DeepSeek-Reasonix architecture](https://github.com/esengine/DeepSeek-Reasonix/blob/main/docs/ARCHITECTURE.md).
- [DeepSeek-Reasonix CLI reference](https://github.com/esengine/DeepSeek-Reasonix/blob/main/docs/CLI-REFERENCE.md).

Reasonix is MIT licensed. DeepSeekCode should treat it as design inspiration
and test-case inspiration. Do not vendor or copy source implementation unless a
future change explicitly carries the required license notice and review.

## What To Absorb

### Cache-First Loop

Reasonix treats DeepSeek prompt caching as a first-class architecture constraint:
stable prompt prefixes, append-only logs, cache-hit telemetry, and cache-safe
compaction are designed together.

DeepSeekCode already stores provider-reported cache hit/miss tokens and
estimated cost in runtime usage records, and the TUI has `/cache`, `/cost`, and
usage panels. The gap is not raw telemetry; the gap is making prompt-prefix
stability measurable and deliberate.

Absorb:

- named prompt layers with stable hashes;
- append-only conversation invariants;
- cache-safe compaction thresholds;
- per-turn cache hit ratio and prefix diagnostics;
- user-visible cache/cost status in CLI and TUI surfaces.

### Tool-Call Repair

Reasonix has a repair pipeline for DeepSeek-style tool-call failures:

- flatten deep or wide tool schemas before sending them to the model, then
  re-nest arguments before dispatch;
- scavenge valid tool calls that appear in reasoning or text instead of the
  formal tool-call channel;
- repair truncated JSON arguments when the partial object is recoverable;
- suppress repeated identical tool-call storms.

DeepSeekCode already supports OpenAI-compatible and Anthropic-compatible tool
calls, same-turn batch tool calls, and repeat-call detection in the agent loop.
The main gap is a systematic repair module before parser failures become hard
model failures.

Absorb:

- a bounded, allowlisted repair pipeline;
- repair notes that are observable in runtime events;
- mutating-aware storm detection;
- focused tests for malformed, truncated, scavenged, and repeated calls.

### Cost-Aware Model Routing

Reasonix uses DeepSeek model economics directly: flash-first defaults, pro as a
visible escalation, `/pro` for the next turn, and budget-aware session behavior.

DeepSeekCode already has DeepSeek V4 pricing and usage cost estimates. The gap
is policy: there is no first-class `flash | auto | pro` preset with visible
failure-triggered escalation.

Absorb:

- explicit model preset config;
- one-turn pro arming;
- visible auto-escalation on hard failure signals;
- budget caps and warnings tied to runtime usage records.

### Parallel Read Dispatch

Reasonix marks tools as `parallelSafe` and runs only safe read-style batches in
parallel. Writes remain serial barriers.

DeepSeekCode already preserves same-turn batch tool calls. The next step is to
execute read-only, side-effect-free batches concurrently while preserving
deterministic output order.

Absorb:

- opt-in tool metadata for read-only and parallel-safe execution;
- configurable max concurrency;
- serial barriers around writes, approvals, shell jobs, and MCP calls unless
  explicitly marked safe.

### Operator Evidence Surfaces

Reasonix has `stats`, `diff`, and replay-oriented transcript tools that make
cache/cost behavior easy to inspect.

DeepSeekCode already persists runtime events and usage records. The gap is a
small CLI layer that turns the data into user-facing evidence.

Absorb:

- `deepseek stats` for per-thread/session cache and cost;
- `deepseek events diff` or similar transcript comparison;
- replay-friendly event summaries for demos and regression investigations.

## DeepSeekCode Design

### 1. Prompt Layers And Cache Diagnostics

Introduce a prompt-layer model inside the request builder. This does not require
changing provider APIs immediately; it can start as internal metadata around the
existing `ModelRequest`.

Proposed internal shape:

```text
PromptLayer {
  name: system_static | workspace_instructions | user_memory | tool_catalog |
        session_summary | append_only_turns | volatile_scratch
  text_sha256: string
  bytes: number
  estimated_tokens: number
  cache_stable: boolean
}
```

Runtime storage should persist hashes, byte counts, token estimates, and cache
hit/miss totals. It should not persist full prompt text unless the existing
thread transcript already contains that text.

Initial behavior:

- keep system and tool catalog bytes stable across turns when config has not
  changed;
- append new turns instead of rewriting historical observations;
- when compacting, append a summary record and keep pinned instructions/user
  memory outside the summary;
- show cache diagnostics through `/cache inspect`, `deepseek stats`, and release
  evidence commands.

Suggested thresholds, configurable later:

- show a context warning around 50%;
- compact older turns around 75%;
- force summary/chunking behavior around 85%;
- never silently discard pinned instructions, user memory, approval state, or
  active task state.

### 2. Tool-Call Repair Pipeline

Add a `src/model/tool_repair.rs` module with a narrow public API:

```text
repair_tool_calls(raw_response, known_tools, tool_schemas, repair_context)
  -> RepairedToolCalls | NoRepair | RepairFailure
```

Pipeline order:

1. Parse formal provider tool calls normally.
2. If parsing fails or no formal call is present, try scavenge from bounded
   reasoning/text content.
3. If arguments are malformed and the parser reports an unterminated object,
   try truncation repair.
4. If the tool schema was flattened, re-nest dot-path arguments before dispatch.
5. Pass the final calls through storm detection before execution.

Safety rules:

- only allow known registered tool names;
- cap scanned text size, repaired call count, and repaired argument size;
- never infer a mutating tool call from vague prose;
- record repair notes into runtime events and debug logs;
- failed repair should return a clear model-facing observation rather than
  panic or silently finish.

Tool schema flattening can be introduced behind a config flag first:

```text
model.tool_schema_flattening = auto | off
```

Flattening trigger:

- schema depth greater than 2; or
- more than 10 leaf parameters.

### 3. Model Presets, Pro Escalation, And Budgets

Add a user-facing model preset separate from the raw model id:

```text
deepseek config preset auto
deepseek config preset flash
deepseek config preset pro
deepseek run --preset auto "..."
```

Preset semantics:

- `flash`: use `deepseek-v4-flash` unless the user explicitly overrides model id;
- `pro`: use `deepseek-v4-pro`;
- `auto`: default to flash, escalate visibly to pro for the current or next turn
  when failure signals cross a threshold.

Candidate auto-escalation signals:

- tool-call repair fired repeatedly in the same turn;
- malformed tool calls after repair;
- repeated identical tool-call storm;
- search/list/read attempts repeatedly find nothing;
- tests fail after the agent already edited relevant files;
- the model emits no actionable tool call and no final answer for multiple
  steps.

Escalation must be visible:

```text
model preset: auto
escalating next call to deepseek-v4-pro: repeated malformed tool arguments
```

Add one-turn pro arming:

```text
/pro
/pro off
deepseek run --pro-next "..."
```

Budget design:

- store optional session budget in runtime thread/session metadata;
- warn at 80%;
- refuse new model turns at 100% unless the user raises or disables the budget;
- use existing micro-USD estimates from runtime usage records.

### 4. Parallel-Safe Tool Dispatch

Extend the `Tool` trait or registry metadata with:

```text
read_only: bool
parallel_safe: bool
storm_exempt: bool
```

Defaults should be conservative: all false unless a tool opts in.

Initial parallel-safe candidates:

- `list_files`;
- `read_file`;
- `search_text`;
- `git_diff` and `git_status` read-only forms;
- read-only runtime queries;
- read-only MCP/resource calls only after they opt in explicitly.

Do not parallelize:

- file writes or patches;
- rollback/revert;
- shell commands and tests;
- approvals or user-input requests;
- side-effect MCP calls;
- tools that depend on prior output from the same model turn.

Dispatch algorithm:

1. Keep the original model call order.
2. Split same-turn batch calls into contiguous chunks.
3. Run a chunk concurrently only when every call in it is `parallel_safe`.
4. Preserve output order when creating observations.
5. Stop or downgrade to serial when cancellation, approval, or policy errors
   occur.

Config:

```text
DSCODE_PARALLEL_MAX=4
DSCODE_TOOL_DISPATCH=auto|serial
```

### 5. Stats, Diff, And Replay

Add a small CLI layer over existing runtime records:

```text
deepseek stats
deepseek stats --session <name>
deepseek stats --thread <id>
deepseek events diff <left-thread> <right-thread>
deepseek events replay <thread>
```

Minimum `stats` output:

- turns;
- prompt tokens and completion tokens;
- prompt cache hit/miss tokens and hit rate;
- input/output/total estimated cost;
- current preset/model split;
- repair count and repeated-tool suppressions once those events exist.

Minimum `diff` output:

- total cost delta;
- cache hit-rate delta;
- tool call count delta;
- failed tool call delta;
- files modified delta when available.

This makes performance claims and demo regressions inspectable without reading
raw runtime JSON.

## Phased Plan

### Phase 1: Repair Pipeline

Deliver:

- `tool_repair` module;
- truncation repair and scavenge for known tool names;
- schema flatten/re-nest behind `model.tool_schema_flattening=auto`;
- repair runtime events;
- unit tests for malformed JSON, truncated JSON, scavenged calls, and unknown
  tool rejection.

Reason to start here: it directly improves task success when DeepSeek emits
almost-correct tool calls.

### Phase 2: Prompt Layer Diagnostics

Deliver:

- prompt-layer hashes and token estimates;
- runtime usage linkage to prompt-layer metadata;
- `/cache inspect` enhancement;
- `deepseek stats` MVP.

Reason: it turns existing cache telemetry into actionable cache-first behavior.

### Phase 3: Model Presets And Budgets

Deliver:

- `preset = auto | flash | pro` config;
- CLI/TUI commands for preset and `/pro`;
- visible auto-escalation;
- session budget warning/refusal.

Reason: it gives users predictable cost/performance controls while preserving
DeepSeek-first defaults.

### Phase 4: Parallel Read Dispatch

Deliver:

- tool metadata;
- same-turn read-only parallel chunks;
- output-order preservation;
- serial fallback;
- cancellation tests.

Reason: this speeds up exploration without changing write safety.

### Phase 5: Evidence And Polish

Deliver:

- `deepseek events diff` and replay summaries;
- dogfood evidence comparing before/after repair and cache behavior;
- README/current-status updates once behavior is verified.

Reason: public claims should be backed by observable runtime data.

## Acceptance Criteria

- malformed but recoverable tool calls no longer fail the turn silently;
- every repaired call creates an observable repair event;
- cache hit/miss and prefix-layer diagnostics are visible without raw JSON;
- pro-tier escalation is never silent;
- parallel dispatch never runs mutating tools concurrently;
- `node scripts/check-secrets.js` and focused Rust tests cover the new paths;
- public docs describe the feature as DeepSeekCode behavior, not copied
  Reasonix behavior.
