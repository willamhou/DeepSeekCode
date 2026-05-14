# DeepSeek-TUI Shell Supervisor Template Capability

**Status:** implemented
**Comparison source:** `Hmbown/DeepSeek-TUI` refreshed at `/tmp/deepseek-tui-compare-20260514`; latest fetched `origin/main` `13e7957621448792beda06ec8615e33cb374adce`.

## Gap

DeepSeekCode's shell supervisor protocol now supports workspace-local
`health/status/show/start/wait/replay/attach/stdin/resize/cancel/shutdown`
requests and can own native-supervisor PTY jobs on supported Unix/Linux builds.
The generated systemd and launchd service templates still carried the older
comment that native PTY sessions were not implemented, which made the packaged
service surface look behind the actual protocol.

## Implementation

- Updated systemd shell-supervisor service comments to describe
  native-supervisor PTY jobs where supported.
- Updated launchd shell-supervisor plist comments with the same capability
  wording.
- Replaced the stale unknown-method error text that tied all unsupported
  methods to pre-native-PTY support.
- Added regression assertions that service templates contain the current
  capability wording and no longer emit the stale native-PTY limitation.

## Verification

- `/home/willamhou/.cargo/bin/cargo fmt --check`
- `/home/willamhou/.cargo/bin/cargo test service_templates_render_runtime_and_agent_supervisors --lib`
- `/home/willamhou/.cargo/bin/cargo test shell_supervisor_protocol_reports_unsupported_unknown_method --lib`
- `/home/willamhou/.cargo/bin/cargo check`
- `/home/willamhou/.cargo/bin/cargo test --lib -- --test-threads=1`
- `git diff --check`

## Remaining

This closes the public template accuracy gap. It does not add new PTY behavior
or broaden platform proof beyond the existing Unix/Linux native-supervisor
coverage.
