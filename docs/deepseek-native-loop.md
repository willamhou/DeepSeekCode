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

## Current Residual Gap Snapshot

The first DeepSeek-native loop slice has landed: repair, prompt-layer
diagnostics, presets/budgets, parallel read dispatch, and stats/replay evidence
all have code paths and deterministic tests. The remaining loop gaps are now
hardening gaps rather than architecture blockers:

- Cache-first behavior is observable and locally enforceable. Prompt-layer
  hashes, token estimates, cache hit/miss usage, configurable daemon
  compaction thresholds, and per-layer trend analysis are recorded. `deepseek
  stats --require-prefix-stable` can fail CI/dogfood checks when prompt-layer
  evidence is missing or stable prompt-prefix layers change hash.
- Tool-call repair has deterministic coverage, but it needs more live
  DeepSeek-backed examples across real gateways, dynamic MCP schemas, and
  non-recoverable malformed-call recovery paths before treating it as mature.
- Model presets and session budgets work, but auto-escalation remains an
  initial heuristic. It still needs dogfood calibration against real failure
  modes and clearer user override/raise-budget flows.
- Parallel dispatch is deliberately conservative. Built-in local read tools and
  common runtime query tools now cover the initial and extended safe set;
  read-only MCP/resource tools should be added incrementally after each surface
  proves side-effect free. Parallel chunk telemetry is recorded on tool result
  events through `meta.parallel_*` lines.
- Deterministic repair/cache evidence and prompt-prefix stability now run in
  the release matrix and are uploaded as loop evidence artifacts. The remaining
  evidence gap is recurring live model-backed dogfood across real gateways and
  dynamic MCP/resource surfaces.

## What To Absorb

### Cache-First Loop

Reasonix treats DeepSeek prompt caching as a first-class architecture constraint:
stable prompt prefixes, append-only logs, cache-hit telemetry, and cache-safe
compaction are designed together.

DeepSeekCode already stores provider-reported cache hit/miss tokens and
estimated cost in runtime usage records, and the TUI has `/cache`, `/cost`, and
usage panels. Prompt-prefix stability is now measurable through prompt-layer
hashes, trend stats, a fail-closed `--require-prefix-stable` gate, and
configurable daemon compaction thresholds.

Absorb:

- named prompt layers with stable hashes;
- append-only conversation invariants;
- cache-safe compaction thresholds;
- per-turn cache hit ratio and prefix diagnostics;
- automated prefix-stability regression gates;
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
It now has a systematic repair module before parser failures become hard model
failures; the remaining work is live DeepSeek-backed evidence across more
gateways and malformed-call edge cases.

Absorb:

- a bounded, allowlisted repair pipeline;
- repair notes that are observable in runtime events;
- mutating-aware storm detection;
- focused tests for malformed, truncated, scavenged, and repeated calls.

### Cost-Aware Model Routing

Reasonix uses DeepSeek model economics directly: flash-first defaults, pro as a
visible escalation, `/pro` for the next turn, and budget-aware session behavior.

DeepSeekCode already has DeepSeek V4 pricing, usage cost estimates, first-class
`flash | auto | pro` presets, visible escalation, and runtime budget records.
The remaining work is dogfood calibration of the auto-escalation heuristic and
clearer user raise-budget flows.

Absorb:

- explicit model preset config;
- one-turn pro arming;
- visible auto-escalation on hard failure signals;
- budget caps and warnings tied to runtime usage records.

### Parallel Read Dispatch

Reasonix marks tools as `parallelSafe` and runs only safe read-style batches in
parallel. Writes remain serial barriers.

DeepSeekCode now executes opt-in local read and runtime query batches
concurrently while preserving deterministic output order and recording
`meta.parallel_*` telemetry. The next boundary is explicit opt-in for
read-only MCP/resource surfaces after side-effect safety is proven.

Absorb:

- opt-in tool metadata for read-only and parallel-safe execution;
- configurable max concurrency;
- serial barriers around writes, approvals, shell jobs, and MCP calls unless
  explicitly marked safe.

### Operator Evidence Surfaces

Reasonix has `stats`, `diff`, and replay-oriented transcript tools that make
cache/cost behavior easy to inspect.

DeepSeekCode already persists runtime events and usage records and exposes them
through `deepseek stats`, `deepseek events replay`, `deepseek events diff`, and
deterministic repair/cache dogfood evidence. The remaining work is making that
evidence recurring in release operations alongside live dogfood runs.

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
- repair count and repeated-tool suppressions once those events exist;
- per-layer prompt trend output for token deltas, hash changes, and
  cache-stable-layer hash-change totals;
- `--require-prefix-stable` failure gate for cache-stable prompt-layer hash
  regressions.

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

Status on 2026-05-24: initial repair pipeline landed. DeepSeekCode now repairs
recoverable truncated JSON tool arguments, scavenges explicit JSON-shaped tool
calls from assistant reasoning/text when formal provider tool calls are absent,
rejects unknown tool names, rejects trailing JSON garbage in repaired tool
arguments, flattens nested object tool schemas behind
`model.tool_schema_flattening = "auto"` and re-nests flat arguments before tool
dispatch, emits visible repair notes, persists structured `tool_call_repair`
runtime events, and surfaces repair evidence in the TUI/runtime stream. Storm
detection is now mutating-aware: read-only calls get one warning retry, while
mutating or unknown calls are suppressed before the second identical execution.

Deliver:

- `tool_repair` module; landed;
- truncation repair and scavenge for known tool names; landed;
- schema flatten/re-nest behind `model.tool_schema_flattening=auto`; landed;
- repair runtime events; landed as structured `tool_call_repair` events,
  runtime stream items, and `exec --json` repair notices;
- unit tests for malformed JSON, truncated JSON, scavenged calls, and unknown
  tool rejection; landed.

Verification:

- `deepseek dogfood repair-cache-evidence --json` writes
  `.dscode/dogfood/repair-cache-evidence.json`, records before/after runtime
  threads, and proves a truncated `read_file` argument object fails strict
  parsing before repair but recovers end to end after repair.

Reason to start here: it directly improves task success when DeepSeek emits
almost-correct tool calls.

### Phase 2: Prompt Layer Diagnostics

Status on 2026-05-24: initial prompt-layer diagnostics landed. DeepSeekCode now
derives named prompt layers with SHA-256 hashes, byte counts, token estimates,
and cache-stability flags for every agent-loop model request. `exec`,
TUI-started agent turns, and runtime daemon task turns persist
`prompt_layers_recorded` events linked to the corresponding usage record.
`/cache inspect` surfaces active-thread prompt-layer snapshot counts, latest
digest, latest token estimate, and layer names when those events exist, and
`deepseek stats` aggregates cache, cost, model split, repair, suppression, and
prompt-layer evidence. Stats also reports per-layer trend lines/JSON for
snapshot count, first/latest/max estimated tokens, token delta, hash-change
count, latest hash, and cache-stable-layer hash-change totals.

Deliver:

- prompt-layer hashes and token estimates; landed;
- runtime usage linkage to prompt-layer metadata for exec, TUI, and daemon task
  turns; landed;
- `/cache inspect` enhancement; landed;
- `deepseek stats` MVP; landed;
- per-layer prompt trend output and cache-stable hash-change totals; landed;
- automated prefix-stability regression gate via
  `deepseek stats --require-prefix-stable`; landed;
- configurable daemon compaction threshold and keep-tail policy via
  `runtime.daemon_compaction_threshold_tokens` and
  `runtime.daemon_compaction_keep_tail_turns`; landed.

Reason: it turns existing cache telemetry into actionable cache-first behavior.

### Phase 3: Model Presets And Budgets

Status on 2026-05-24: initial model preset and budget controls landed.
DeepSeekCode now stores `model.preset = "auto" | "flash" | "pro"` separately
from the raw `model.model` marker, defaults new configs to the `auto` preset,
and exposes `deepseek config preset [auto|flash|pro]`,
`deepseek config budget [MICROUSD|off]`, `deepseek run --preset ...`,
`deepseek exec --preset ...`, and `--pro-next` overrides. The TUI supports
`model preset <auto|flash|pro>` plus `/pro` to arm DeepSeek V4 Pro for the next
submitted user turn. Auto routing emits a visible escalation line/event before
using `deepseek-v4-pro`, and session budget enforcement warns at 80% and refuses
new model calls once the in-loop estimated DeepSeek spend reaches
`model.session_budget_microusd`. Runtime session/thread records now also persist
`session_budget_microusd` from the active config; TUI and daemon task turns
restore prior durable usage cost before entering the agent loop, so budget
warning/refusal survives process restarts while `deepseek config budget off`
clears the runtime limit.

Deliver:

- `preset = auto | flash | pro` config; landed;
- CLI/TUI commands for preset and `/pro`; landed;
- visible auto-escalation; landed for auto routes that select Pro;
- session budget warning/refusal; landed for current agent-loop estimated
  DeepSeek spend and cross-process runtime sessions;
- explicit per-thread/session budget metadata in runtime records; landed.

Reason: it gives users predictable cost/performance controls while preserving
DeepSeek-first defaults.

### Phase 4: Parallel Read Dispatch

Status on 2026-05-24: initial parallel-safe read dispatch landed. The tool
registry now exposes conservative `read_only` and `parallel_safe` metadata.
The agent loop splits same-turn batches into contiguous safe chunks and runs
only opt-in read tools concurrently when hooks and permission prompts are not in
play. The parallel-safe local read set is `list_files`, `list_dir`,
`read_file`, `retrieve_tool_result`, `search_text`, `grep_files`,
`file_search`, `git_status`, `git_diff`, `git_log`, `git_show`, `git_blame`,
`project_map`, and `validate_data`; common runtime query tools include
`task_list`, `task_read`, `agent_list`, `agent_result`, `automation_list`,
`automation_read`, `pr_attempt_list`, and `pr_attempt_read`. Results are written
back in the original model-call order, mixed read/write batches fall back to
serial execution at write barriers, `DSCODE_TOOL_DISPATCH=serial` disables the
path, and `DSCODE_PARALLEL_MAX` caps concurrency. Tool events from this path
include `meta.parallel_dispatch`, `meta.parallel_chunk_size`, and
`meta.parallel_elapsed_ms` telemetry.

Deliver:

- tool metadata; landed for registry read-only and parallel-safe flags;
- same-turn read-only parallel chunks; landed for the initial and extended local
  opt-in tool set;
- output-order preservation; landed for observations and tool events;
- serial fallback; landed for writes, shell, approval/user-input, hooks, repeats,
  side-effect MCP calls, and `DSCODE_TOOL_DISPATCH=serial`;
- cancellation tests; landed for pre-dispatch cancellation;
- parallel chunk telemetry; landed on tool result events.

Reason: this speeds up exploration without changing write safety.

### Phase 5: Evidence And Polish

Status on 2026-05-24: initial runtime event replay/diff CLI and repair/cache
dogfood evidence command landed, and the release matrix now runs that evidence
with the prompt-prefix stability gate.
`deepseek events replay <thread>` renders compact chronological runtime event
summaries with stable labels for thread, turn, item, usage, prompt-layer,
permission, goal, and task events. `deepseek events diff <left-thread>
<right-thread>` compares two runtime threads for event count, estimated cost,
prompt cache hit rate, tool calls, failed tool calls, file modification evidence
when paths were recorded, repair events, repeated-tool suppressions, and event
kind deltas. Both commands support `--json` for regression evidence and demos.
`deepseek dogfood repair-cache-evidence` creates a deterministic local
before/after run that exercises `tool_call_repair`, prompt-layer events, cache
hit/miss usage, `events replay`, `events diff`, and `stats`. The Release Matrix
packaging job persists the repair/cache JSON and `stats --require-prefix-stable`
JSON as `deepseek-loop-evidence`.

Deliver:

- `deepseek events diff` and replay summaries; landed;
- dogfood evidence comparing before/after repair and cache behavior; landed via
  `deepseek dogfood repair-cache-evidence --json`;
- recurring release evidence for deterministic repair/cache and prompt-prefix
  stability; landed in the Release Matrix packaging job;
- README/current-status updates once behavior is verified; landed.

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
