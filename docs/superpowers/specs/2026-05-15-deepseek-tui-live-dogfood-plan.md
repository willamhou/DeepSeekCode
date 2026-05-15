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
  - concrete `dogfood replay-benchmark` commands for the next safe batch.
- The planner is read-only by default and supports:
  - `--manifest <path>`;
  - `--target-live-runs <n>`;
  - `--target-live-success-rate <percent>`;
  - repeated `--target-category <category>:<min-runs>:<min-success-percent>`;
  - `--limit <n>` per category;
  - `--json`.

## Verification

- Parser coverage for `dogfood live-plan`.
- Unit coverage for live-plan category recommendations and JSON output.
- `cargo test dogfood --lib -- --test-threads=1`
- `cargo test parses_dogfood_live_plan_subcommand --lib`

## Remaining

This makes the live sampling backlog explicit and scriptable. The remaining
product evidence gap still requires running enough online model-backed dogfood
rows until the strict release gate passes.
