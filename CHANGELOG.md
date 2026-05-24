# Changelog

## 0.1.2 - 2026-05-24

Public beta release for the Linux/macOS local code-agent CLI milestone.

### Added

- `deepseek quickstart` / `deepseek onboarding` for side-effect-free first-run
  checks with stable JSON output.
- `deepseek update release-smoke` for current-platform release binary download,
  checksum, extraction, and install smoke verification.
- Public beta README/docs refresh with focused install, evidence, and promotion
  guidance.
- Reusable external fixture catalog covering Python, Rust, and Node disposable
  repositories.
- Tracked online model-backed external fixture evidence for Python invoice,
  Rust order, and Node task-report samples.

### Changed

- README and install docs now point at the `v0.1.2` release assets and GHCR
  image.
- Linux CI now smoke-tests the full external fixture catalog.
- Release/current-status docs now separate the Linux/macOS CLI milestone from
  broader product-hardening work.

### Verification

- `cargo fmt --check`
- `cargo test --lib -- --test-threads=1`
- `node scripts/check-secrets.js`
- `node npm/scripts/check-version-sync.js`
- `node packaging/homebrew/verify-formula.js`
- Online `deepseek dogfood external-fixture` plus `external-evidence` for Rust
  and Node samples.

## 0.1.1 - 2026-05-14

Release sync for public GitHub Release binaries and GHCR distribution.

### Added

- GitHub Release assets for Linux x64, macOS x64, macOS arm64, and Windows x64.
- GHCR image publishing with version, tag, and latest aliases.
- npm platform package staging metadata and Homebrew formula template checks.

### Verification

- Release Matrix workflow completed for all supported package targets.
- GitHub Release and GHCR image smoke checks passed.
- npm and Homebrew publish jobs remained gated on external credentials.

## 0.1.0 - 2026-05-09

Phase 11 closes the first release-ready agent workflow baseline.

### Added

- `deepseek benchmark` release gate with benchmark expectations, trend gate, and dogfood live gate.
- `deepseek dogfood` workflows for live task recording, report generation, benchmark seed export, and benchmark replay.
- Subagent v2 workflow support with parent mergeback, next-action summaries, and parent todo advancement.
- Release and upgrade documentation for source installs, release binaries, rollback, and completion scripts.

### Changed

- Roadmap and spec status now record the Phase 11 closure state.
- `deepseek` is the primary command name; `dscode` remains a compatibility alias.
- Python `pytest` validation runs now use an isolated bytecode cache per `run_shell` call to avoid stale `.pyc` reuse during same-second retry edits.

### Verification

- `cargo fmt --check`
- `cargo test`
- `deepseek benchmark`
- `deepseek version`
- `deepseek doctor`
- Dogfood replay coverage for Rust write/validate and Python retry write/validate fixtures.
