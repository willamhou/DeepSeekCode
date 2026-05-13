# DeepSeek-TUI Diagnostics JSON Daemon Protocol

## Context

DeepSeek-TUI keeps diagnostics close to its terminal runtime and long-running
workbench. DeepSeekCode already had text CLI diagnostics, warmed post-edit
diagnostics, and an HTTP `/v1/diagnostics` broker, but the standalone
`deepseek diagnostics --watch` worker only printed human text. That made it
hard for a supervisor, TUI, or external agent to consume watch ticks without
running the full HTTP runtime.

## Scope

- Add `deepseek diagnostics --json` for one-shot structured output.
- Add `deepseek diagnostics --watch --json` as a newline-delimited JSON tick
  protocol.
- Preserve existing human text output when `--json` is not passed.
- Emit an explicit skipped tick when `--changed` finds no changed files, with
  `skipped: true`, `files: []`, and `report: null`.
- Reuse the same diagnostics report serializer for CLI JSON and HTTP runtime
  diagnostics.
- Make generated systemd/launchd diagnostics workers use `--json` so logs are
  machine-readable by default.

## Implementation

- `DiagnosticsArgs` now parses `--json`.
- `src/cli/commands/diagnostics.rs` emits:
  - `deepseek.diagnostics.report.v1` for one-shot JSON output.
  - `deepseek.diagnostics.daemon_tick.v1` for watch JSONL output.
- `DiagnosticReport::to_json_value()` centralizes report serialization for CLI
  and HTTP runtime callers.
- Service templates now run `diagnostics --watch --changed --interval-ms N
  --json`.
- Runtime and install docs describe both schemas and the skipped-tick behavior.

## Verification

- `cargo test diagnostics --lib`
- `cargo test parses_diagnostics_args --lib`
- `cargo test service_templates_render_runtime_and_agent_supervisors --lib`
- `cargo fmt --check`
- `git diff --check`
- `cargo run --quiet -- diagnostics --watch --once --json README.md`

## Remaining

This is a stdout JSONL protocol, not a separate diagnostics socket server. The
HTTP runtime broker remains the socket-based integration point.
