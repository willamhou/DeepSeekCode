# DeepSeek-TUI Live Dogfood Plan

**Status:** implemented
**Comparison source:** `Hmbown/DeepSeek-TUI` refreshed at
`/tmp/deepseek-tui-compare-20260514`, `origin/main`
`b8345488978265cd94990364edcdefbb21bc5f15`.

## Gap

The live dogfood gates can now distinguish model-backed rows from offline
fallback rows, but the operator still had to manually inspect the ledger and
benchmark manifest to decide which category to run next. That slows down the
remaining 100+ live sample collection needed before claiming a smaller
Claude/Codex/DeepSeek-TUI gap.

## Implementation

- Added `deepseek dogfood live-plan`.
- The command reads the dogfood ledger and benchmark manifest, then reports:
  - current `model_transport` for the configured model;
  - total model-backed run/success progress;
  - per-category live progress against default targets
    `write_validate:25:90`, `recovery:25:90`, and `pr_workflow:25:90`;
  - replayable unique benchmark cases for each category;
  - concrete `dogfood live-run` dry-run and `--execute` commands for the next
    safe model-backed batch;
  - a `post_run_report_command` plus structured `evidence_gate` requirements
    for the release operator to run after online execution.
- The planner is read-only by default and supports:
  - `--manifest <path>`;
  - `--target-live-runs <n>`;
  - `--target-live-success-rate <percent>`;
  - repeated `--target-category <category>:<min-runs>:<min-success-percent>`;
  - `--limit <n>` per category;
  - `--json`.
- `dogfood live-run --json` emits a machine-readable dry-run execution plan
  with selected cases, online readiness, the exact follow-up `--execute`
  command, and the exact post-run `dogfood report --require-live-*` gate. It is
  intentionally dry-run only; online execution uses `dogfood live-run --execute`
  without `--json` so progress logs do not corrupt structured output.
- `dogfood live-run --api-key-file <path>` / `--key-file <path>` loads a
  repository-external key file into the configured `model.api_key_env` for the
  current process only, restores the previous environment value on return, and
  preserves the flag in dry-run and execute follow-up commands without printing
  the key value.
- `dogfood live-run --execute --evidence-out <path>` writes a
  `deepseek.dogfood.live_run_evidence.v1` JSON summary after the batch ends or
  after the first failed case. The summary records before/after ledger live
  counts, selected cases, appended model-backed rows, per-case outcomes, the
  benchmark gate result, the post-run report gate command, and a ledger file
  `fnv1a64` fingerprint without storing the API key value.
- `dogfood live-evidence --file <path>` verifies that summary as a fail-closed
  release gate. By default it requires the evidence kind to match, the batch to
  be completed and online, at least one appended model-backed row, case evidence,
  and a post-run report command with live gates. `--require-benchmark-gate`
  additionally requires the benchmark gate to pass, and `--json` emits
  `deepseek.dogfood.live_evidence_verification.v1`.
- `dogfood live-evidence --require-report-gate` reads the structured
  `evidence_gate` requirements and the ledger path from the evidence file, then
  applies the same `dogfood report` live requirement logic without executing the
  shell command string embedded for operator convenience. It also recomputes the
  ledger fingerprint and fails on evidence/ledger mismatch.
- The report-gate verifier also checks each appended case evidence row against
  the ledger by timestamp, outcome, model transport, and benchmark category, so
  a stale or tampered evidence file cannot pass only because aggregate live
  counts are high enough.
- `dogfood live-evidence --out <path>` writes the
  `deepseek.dogfood.live_evidence_verification.v1` result to disk, making the
  verifier output uploadable as a release evidence artifact.

## Verification

- Parser coverage for `dogfood live-plan`.
- Unit coverage for live-plan category recommendations and JSON output.
- Text and JSON output recommend `dogfood live-run`, not the offline-friendly
  `replay-benchmark` path, so release operators collect model-backed evidence.
- Text and JSON output include a post-run evidence gate command, so the online
  batch has a machine-checkable model-backed acceptance step instead of relying
  on manual ledger inspection.
- `dogfood live-run --json` has parser/unit/CLI coverage for structured dry-run
  planning before spending online model calls.
- Unit coverage verifies key-file command preservation, secret redaction, and
  environment restoration.
- Unit coverage verifies live-run evidence summary before/after deltas,
  appended model-backed row counts, benchmark gate status, ledger fingerprint
  binding, file writing, and secret redaction.
- Parser and unit coverage verify `dogfood live-evidence`, including the
  default online/completed/model-backed requirements and stricter appended-row
  failures.
- Unit coverage verifies the structured report gate extraction and successful
  ledger check from a live evidence summary.
- Unit coverage verifies tampered case timestamps fail ledger matching.
- Unit coverage verifies tampered ledger fingerprints fail report-gate
  verification.
- Unit coverage verifies verification JSON artifact writing through the
  `live-evidence --out` path.
- `cargo test dogfood --lib -- --test-threads=1`
- `cargo test parses_dogfood_live_plan_subcommand --lib`

## Remaining

This makes the live sampling backlog explicit and scriptable. The remaining
product evidence gap still requires running enough online model-backed dogfood
rows until the strict release gate passes.
