# DeepSeek-TUI Live Dogfood Gates

**Status:** implemented
**Comparison source:** `Hmbown/DeepSeek-TUI` refreshed at
`/tmp/deepseek-tui-compare-20260514`, `origin/main`
`b8345488978265cd94990364edcdefbb21bc5f15`.

## Gap

The remaining `<10%` CLI-readiness blocker is not another deterministic replay
case; it is enough true model-backed dogfood evidence. The existing dogfood
report could require total rows and category success rates, but offline fallback
replays could satisfy those counters. That made the 100+ live CLI dogfood target
weaker than the Claude/Codex-style readiness bar.

## Implementation

- New dogfood records store `model_transport`:
  - `online` when the configured API-key environment variable is present and is
    not an explicit offline sentinel;
  - `offline` when the run is using the built-in fallback planner;
  - `unknown` for legacy ledger rows without the field.
- `deepseek dogfood report` now includes a `Model-backed runs` summary line and
  a `Transport` column in the recent-run table.
- `deepseek dogfood report` now supports fail-closed live gates:
  - `--require-live-runs <n>`;
  - `--require-live-success-rate <percent>`;
  - `--require-live-category <category>:<min-runs>:<min-success-percent>`.
- README, install docs, release docs, and the active parity plan use live gates
  in the release-readiness command.

## Verification

- Parser coverage for the new report flags.
- Report rendering coverage for `Model-backed runs` and the table transport
  column.
- Requirement coverage for both passing and failing live-run/category gates.

## Remaining

This closes the auditability gap, not the evidence-volume gap. The product still
needs actual online dogfood rows until the release command passes with 100+
model-backed runs, 25+ `write_validate`, 25+ `recovery`, and 25+ `pr_workflow`
model-backed category rows at or above 90% success.
