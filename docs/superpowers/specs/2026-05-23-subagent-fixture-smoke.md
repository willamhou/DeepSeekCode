# Phase 12D Subagent Fixture Smoke

Date: 2026-05-23

## Goal

Close the Phase 12D subagent hardening gap with an explicit local gate for
parallel subagent request parsing, write-scope coordination, parent readback,
blocker summaries, and conflict summaries.

## Scope

- `dispatch_subagent` accepts optional `write_scope` / `write_set`.
- `dispatch_subagents` accepts per-child `write_scope` / `write_set` in the
  `tasks` JSON array.
- Parallel summaries expose aggregate readback and coordination metadata.
- Parent planner consumes both single-child and parallel-child summaries.
- `deepseek agents subagent-fixture-smoke --json` verifies the local contract.

## Parallel Contract

Each parallel child task may include:

```json
{
  "task": "Review src/api.rs for correctness risks",
  "agent": "reviewer",
  "skill": "pr-review",
  "write_scope": "src/api.rs",
  "steps": "4"
}
```

`write_set` is accepted as an alias for `write_scope`.

The child prompt includes the assigned write scope and instructs the child to
report a blocker rather than editing outside that scope. This is a coordination
contract, not a filesystem sandbox.

## Summary Metadata

Parallel summaries include:

```text
meta.parallel_children=2
meta.parallel_blocked_children=1
meta.parallel_readback_required=true
meta.parallel_next_action=read_file:src/cli/app.rs
meta.parallel_child_files=src/cli/app.rs,docs/agents.md
meta.parallel_child_1_write_scope=src/cli/app.rs
meta.parallel_child_1_files=src/cli/app.rs
meta.parallel_write_scope_conflicts=src/cli/app.rs
```

When a child reports files, the parent planner reads them back before relying
on child edits. Blocked child summaries trigger parent replanning hints.

## Fixture Gate

`deepseek agents subagent-fixture-smoke --json` creates a temporary `.dscode`
root and verifies:

- parser accepts two parallel tasks with disjoint write scopes
- disjoint write scopes produce no conflict summary
- parallel summaries require parent readback when child files are present
- blocked child summaries report aggregate blocker count
- overlapping write scopes report `meta.parallel_write_scope_conflicts`
- thread artifacts include the write scope and child file metadata

JSON kind:

```json
{
  "kind": "deepseek.subagent_fixture_smoke.v1",
  "parser_ok": true,
  "disjoint_write_scope_ok": true,
  "readback_required_ok": true,
  "blocker_summary_ok": true,
  "conflict_summary_ok": true,
  "artifact_ok": true,
  "child_count": 2
}
```

## Acceptance Evidence

Local commands run on 2026-05-23:

```bash
cargo test tools::dispatch_subagent --lib
cargo test model::deepseek::tests::offline_planner_reads_child_file_after_parallel_subagent_summary --lib
cargo test model::deepseek::tests::child_file_paths_include_parallel_child_metadata --lib
cargo test cli::app::tests::cli_from_argv_routes_agents_subcommands --lib
cargo run --quiet -- agents subagent-fixture-smoke --json
```

Observed smoke result:

- `parser_ok = true`
- `disjoint_write_scope_ok = true`
- `readback_required_ok = true`
- `blocker_summary_ok = true`
- `conflict_summary_ok = true`
- `artifact_ok = true`
- `child_count = 2`

## Benchmark Evidence

The default benchmark manifest contains 20 `subagent` category cases.

The benchmark runner now supports targeted selection:

- `--category <name>`
- repeatable `--case <name>`

Filtered runs write a markdown report but do not advance benchmark history or
enforce the full trend/live gates, so short slice evidence cannot pollute the
release baseline.

Current local evidence:

- `cargo run --quiet -- benchmark --category subagent --out /tmp/deepseek-subagent-benchmark.md`
- Result: `20/20`
- Report: `/tmp/deepseek-subagent-benchmark.md`
- Trend gate: `skipped (filtered benchmark selection)`
- Live gate: `skipped (filtered benchmark selection)`
- `cargo run --quiet -- benchmark`
- Result: `82/82`
- Report: `.dscode/benchmarks/latest.md`
- Trend gate: `skipped (need at least 3 prior comparable runs, found 2)`
- Live gate: `pass against previous dogfood snapshot (runs 5 -> 20)`

The remaining evidence pass is broader online model-backed dogfood, not another
local subagent smoke gap.
