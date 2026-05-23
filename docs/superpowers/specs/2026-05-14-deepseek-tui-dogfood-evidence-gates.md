# DeepSeek-TUI Dogfood Evidence Gates

**Status:** implemented
**Comparison source:** `Hmbown/DeepSeek-TUI` refreshed at `/tmp/deepseek-tui-compare-20260514`, HEAD `9483248a9f35b5f2b56c34b5b84fbc5334473c9d`.

## Gap

DeepSeekCode already records dogfood runs and external write-fixture rows, but
release/product readiness could still be described manually from a report. For
the remaining Claude Code CLI / Codex CLI / DeepSeek-TUI polish gap, evidence
needs to fail closed when live samples are too thin.

## Implementation

- `deepseek dogfood report` now accepts strict evidence gates:
  - `--require-min-runs <n>`
  - `--require-success-rate <percent>`
  - `--require-live-runs <n>`
  - `--require-live-success-rate <percent>`
  - `--require-external-write-fixtures <n>`
  - `--require-recent-clean <n>`
  - `--require-category <category>:<min-runs>:<min-success-percent>`
  - `--require-live-category <category>:<min-runs>:<min-success-percent>`
- The command still writes the Markdown report first, then exits with a
  grouped error when any evidence gate fails.
- New dogfood ledger rows include `model_transport` as `online` only when the
  configured non-offline API key environment variable is present; legacy rows
  read as `unknown` and do not count toward live gates.
- `deepseek dogfood external-fixture` now also fails closed before real
  execution when the current model transport is not `online`. `--dry-run`
  remains available for planning, and `--allow-offline` is explicit
  rehearsal-only behavior that does not satisfy release evidence gates.
- `dogfood external-fixture --evidence-out <path>` writes
  `deepseek.dogfood.external_fixture_evidence.v1` with the source workdir,
  appended external-fixture ledger row(s), model transport, release-evidence
  readiness, and a ledger `fnv1a64` fingerprint for artifact upload.
- The release docs, install docs, and multilingual README surface the strict
  readiness command used for the 100 model-backed-run / overall 90% /
  25-per-category / 90% success-rate dogfood target.

## Verification

- `cargo test parses_dogfood_report_subcommand --lib`
- `cargo test dogfood_report_rejects_invalid_evidence_gate --lib`
- `cargo test report_requirements_pass_with_external_and_category_evidence --lib`
- `cargo test external_fixture_requires_online_transport_unless_rehearsal --lib`
- `cargo test external_fixture_evidence_summary_records_release_ready_row --lib`
- `cargo test report_requirements_fail_on_missing_live_evidence --lib`
- `cargo test dogfood --lib -- --test-threads=1`
- `cargo fmt --check`
- `cargo check`
- `cargo test --lib -- --test-threads=1`
- `git diff --check`

## Remaining

The gate proves readiness only when enough live rows exist. The actual remaining
product gap is still to run and record enough online/external write-fixture
samples across disposable repositories, plus the separate model-backed README
demo recording.
