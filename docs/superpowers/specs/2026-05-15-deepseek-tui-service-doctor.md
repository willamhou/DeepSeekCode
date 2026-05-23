# DeepSeek-TUI Service Doctor

## Context

DeepSeek-TUI v0.8.37 added Tencent Lighthouse deployment assets and a
deployment doctor script that checks runtime binaries, environment, service
units, and localhost health before treating a remote bridge as ready.
DeepSeekCode already renders systemd and launchd templates through
`deepseek agents service`, but it only had a render smoke check. Operators did
not have a repo-native preflight command for validating that the rendered
service topology still matches the selected binary, workspace, and supervisor
commands.

## Spec

- Add `deepseek agents service-doctor`.
- Reuse the same `--kind`, `--out`, `--bin`, `--workdir`, `--addr`,
  `--interval-ms`, and `--budget` inputs as `agents service`.
- Add `--json` for CI/release evidence.
- Validate the selected binary and workspace without starting services.
- Validate that the generated template set includes runtime, agents,
  diagnostics, and shell-supervisor services for the selected service manager.
- Parse generated systemd `ExecStart`/`WorkingDirectory` and launchd
  `ProgramArguments`/`WorkingDirectory` back into exact argv/workdir vectors
  and compare them to the expected runtime, agents daemon, diagnostics watch,
  and shell-supervisor commands.
- When `--out` is supplied, verify that the on-disk service files and
  `SERVICES.md` exist and match the current render output.
- Add `--installed` as a read-only installed service gate: systemd checks the
  four user units with `systemctl --user show`; launchd checks the four labels
  with `launchctl print gui/<uid>/<label>`. Missing, inactive, failed, or
  disabled installed services are blockers.
- Report blockers and warnings separately. Missing platform service managers
  are warnings; stale or missing explicit `--out` files are blockers.
- Update service and release docs plus the parity plan.

## Verification

- `cli_from_argv_routes_agents_service_doctor`
- `service_doctor_reports_generated_service_health`
- `service_doctor_detects_stale_generated_template`
- `service_template_command_vectors_handle_quoted_paths`
- `service_doctor_parses_systemd_installed_status`
- `service_doctor_parses_launchd_installed_status`
- `cargo test service_doctor --lib`
- `cargo fmt --check`
- `cargo check`
- `cargo build --bin deepseek`
- `target/debug/deepseek agents service --kind systemd --out target/service-doctor-smoke --bin target/debug/deepseek --workdir "$PWD"`
- `target/debug/deepseek agents service-doctor --kind systemd --out target/service-doctor-smoke --bin target/debug/deepseek --workdir "$PWD" --json`
- `cargo test --lib -- --test-threads=1`
- `git diff --check`
