# Changelog

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
