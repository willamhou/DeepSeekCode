# DeepSeek-TUI Live Dogfood Run

**Status:** implemented
**Comparison source:** `Hmbown/DeepSeek-TUI` refreshed at
`/tmp/deepseek-tui-compare-20260514`, `origin/main`
`b8345488978265cd94990364edcdefbb21bc5f15`.

## Gap

`deepseek dogfood live-plan` made the live evidence backlog visible, but the
operator still had to copy per-category replay commands manually. That slows
down the 100+ model-backed sample collection needed before claiming a smaller
Claude/Codex/DeepSeek-TUI gap.

## Implementation

- Added `deepseek dogfood live-run`.
- The command reuses the live-plan target model and selects the next recommended
  benchmark cases across `write_validate`, `recovery`, and `pr_workflow`.
- Unfiltered batches are category-balanced: selection walks the active
  categories round-robin, then fills the next round when a category has more
  recommended cases.
- It is safe by default:
  - dry-run unless `--execute` is present;
  - default batch limit is 3;
  - repeated `--category <name>` filters the selected categories;
  - `--execute` refuses to run unless the current model transport is `online`.
- It supports the same target-shaping flags as `live-plan`:
  - `--manifest <path>`;
  - `--target-live-runs <n>`;
  - `--target-live-success-rate <percent>`;
  - repeated `--target-category <category>:<min-runs>:<min-success-percent>`.
- Executed batches can use `--evidence-out <path>` to write
  `deepseek.dogfood.live_run_evidence.v1` JSON with before/after ledger live
  counts, appended model-backed rows, per-case outcomes, benchmark gate status,
  the post-run report gate command, and a ledger file `fnv1a64` fingerprint.
- `dogfood live-evidence --file <path>` verifies that evidence file and defaults
  to completed, online, and at least one appended model-backed ledger row.
- `dogfood live-evidence --require-report-gate` also verifies the evidence file's
  structured live gate against the current ledger, rechecks the ledger
  fingerprint, and matches appended case rows back to ledger records.
- `dogfood live-evidence --out <path>` persists the verification JSON for
  release evidence upload.

## Verification

- Parser coverage for `dogfood live-run`.
- Unit coverage for category filtering, balanced selection, and total run
  limiting.
- Unit coverage for the batch evidence summary JSON and file writer.
- Parser/unit coverage for the batch evidence verifier.
- Command smoke:
  - `deepseek dogfood live-run --limit 3`
  - `deepseek dogfood live-run --limit 2 --category recovery`

## Remaining

This closes the manual command-copy step, but it does not by itself satisfy the
release evidence gate. The ledger still needs enough successful online
model-backed rows for the strict `dogfood report` live thresholds to pass.
