# Changelog

## 0.1.4 - 2026-05-25

npm/npx distribution and public evidence refresh.

### Added

- npm-first README quick start for `npm install -g @deepseek-code/cli` and
  `npx @deepseek-code/cli`.
- Public evidence summary page for demos, repair/cache runtime proof, external
  fixtures, and release/install checks.
- DeepSeek official integration PR draft copy for agent directories and docs.

### Changed

- Bumped Cargo, npm root wrapper, npm platform packages, release-smoke default,
  and Homebrew formula metadata to `0.1.4`.
- Launch copy now treats npm as the primary distribution push for this patch
  release instead of a deferred caveat.
- Release npm publishing now uses absolute tarball paths and a reusable manual
  retry workflow for publishing packages from a completed Release Matrix run.
- `v0.1.4` npm packages are published and verified through npm registry lookup,
  `npx`, and clean-directory install smoke.
- Homebrew tap automation published `Formula/deepseek.rb` for `v0.1.4` and the
  refreshed tap passed macOS x64/arm64 Homebrew Smoke.

### Verification

- `cargo fmt --check`
- `cargo test --lib`
- `npm --prefix npm test`
- `node npm/scripts/check-version-sync.js`
- `node scripts/check-secrets.js`
- manual `NPM Publish` workflow run `26379649992`
- `npx @deepseek-code/cli@0.1.4 version`
- clean-directory `npm install @deepseek-code/cli@0.1.4` smoke
- `Publish Homebrew Tap` job rerun from Release Matrix `26379123804`
- Homebrew Smoke workflow run `26380319039`

## 0.1.3 - 2026-05-24

Linux arm64 distribution hardening.

### Added

- Linux arm64 release matrix target using the native `ubuntu-24.04-arm`
  hosted runner.
- Linux arm64 release asset support in `deepseek update download-plan` and
  `deepseek update release-smoke`.
- Linux arm64 npm platform package metadata and wrapper resolution support.
- Homebrew formula support for Linux arm64 release assets.
- Manual clean-machine release-smoke and Homebrew smoke workflows for public
  release/tap verification evidence.

### Changed

- Release assets now cover Linux x64, Linux arm64, macOS x64, macOS arm64, and
  Windows x64.
- Documentation points at the `v0.1.3` release/install paths.

### Verification

- `cargo fmt --check`
- `cargo test --lib -- --test-threads=1`
- `cargo package --allow-dirty`
- `npm --prefix npm test`
- `node scripts/check-secrets.js`
- `node npm/scripts/check-version-sync.js`
- `node packaging/homebrew/verify-formula.js`
- `target/debug/deepseek update download-plan --version 0.1.3 --platform linux-arm64 --json`

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
