# DeepSeek-TUI Service Smoke

## Context

`deepseek agents service-doctor` closes the static service-template preflight
gap, but the remaining DeepSeek-TUI / Claude Code / Codex parity gap still
calls out service/runtime proof beyond template rendering. DeepSeek-TUI's
Lighthouse deployment doctor checks running processes and localhost health.
DeepSeekCode needs a local, non-installing smoke command that can exercise the
release binary's long-lived surfaces without requiring systemd or launchd.

## Spec

- Add `deepseek agents service-smoke`.
- Accept `--bin`, `--workdir`, `--addr`, `--timeout-ms`, and `--json`.
- Default `--bin` to the current executable, `--workdir` to the current
  directory, `--addr` to `127.0.0.1:0`, and timeout to 5000 ms.
- Start the selected binary as `serve --http --addr <resolved-addr> --once`,
  probe `/health`, and verify the child exits successfully.
- Start the selected binary as `agents shell-supervisor --json`, probe the
  Unix socket `health` method, run a control smoke through `start` -> `wait` ->
  `attach` -> `replay`, request `shutdown`, and verify the child exits.
- On Linux, the shell-supervisor control smoke should request `tty=true` and
  require the `native-supervisor` PTY backend so release evidence proves the
  platform PTY path, then verify PTY `stdin`, `resize`, terminal `replay`, and
  `cancel`, not only non-interactive shell execution.
- On Linux, the same shell-supervisor control smoke should also verify the
  duplex `byte_stream` socket path and `raw_proxy=true` raw byte proxy so the
  bounded PTY proxy slice has release evidence. It now also runs the human
  `deepseek agents shell proxy` raw-mode wrapper under a temporary PTY and
  verifies forwarded input plus terminal-size sync through supervisor replay.
  It also verifies supervisor `pty_fd` fd handoff by receiving a native PTY
  master fd over SCM_RIGHTS, writing through it, observing direct PTY output,
  and confirming normal supervisor stdin/resize/replay resumes after detach.
  Focused coverage also proves CLI `fd-proxy` exits successfully when Ctrl-D
  closes the target PTY and Linux reports master-side `EIO`, and proves local
  SIGWINCH resize forwarding by observing the child PTY's updated `stty size`.
  Ctrl-C forwarding is covered by a PTY SIGINT smoke that observes target-side
  interrupt output through the fd-proxy transcript. Killed-client behavior is
  covered by a SIGKILL smoke that proves the fd lease is released and normal
  supervisor stdin/resize/replay resumes.
- Add `deepseek agents shell-fixture-smoke [--json]` as a focused local
  supervisor-only gate that reuses the shell-supervisor control smoke without
  starting the HTTP runtime surface.
- Strengthen `deepseek agents service-doctor` so the static pre-install gate
  parses generated systemd `ExecStart`/`WorkingDirectory` and launchd
  `ProgramArguments`/`WorkingDirectory` into exact argv/workdir vectors for
  runtime, agents daemon, diagnostics watch, and shell-supervisor templates.
  This remains local evidence and does not replace an actual installed
  systemd/launchd smoke on a clean machine.
- Add `deepseek agents service-doctor --installed` as a read-only installed
  service gate. For systemd it inspects `systemctl --user show` for the four
  generated units and requires loaded/active/enabled state; for launchd it
  inspects `launchctl print gui/<uid>/<label>` for the four labels and requires
  running state. It must not install, enable, restart, stop, or unload services.
- Add `deepseek agents service-smoke --installed --kind <systemd|launchd|all>`
  as a read-only installed runtime smoke. It reuses the installed service
  status checks, requires a concrete `--addr` port, probes the actual runtime
  `/health`, probes the existing workspace shell-supervisor endpoint, and runs
  shell-supervisor control checks without sending `shutdown`. Unix probes
  `.dscode/shell-supervisor/supervisor.sock`; Windows reads
  `.dscode/shell-supervisor/supervisor.tcp` and connects to the recorded
  loopback TCP endpoint.
- Treat an already-active shell-supervisor socket as a blocker so the smoke
  command never shuts down an existing workspace supervisor.
- Treat a too-long absolute shell-supervisor socket path as a blocker before
  spawning the child process, with guidance to use a short isolated `--workdir`
  such as `/tmp/dsc-smk`.
- Keep platforms without Unix socket or Windows TCP shell-supervisor support as
  a warning rather than a blocker.
- Return non-zero when blockers are found.
- Update release/service docs and the parity plan so release evidence can
  include the local smoke output before clean-machine service installation.

## Verification

- `cli_from_argv_routes_agents_service_smoke`
- `service_smoke_resolves_ephemeral_loopback_addr`
- `service_smoke_json_reports_blockers_and_warnings`
- `service_smoke_installed_requires_concrete_addr`
- `service_smoke_shell_supervisor_control_smoke_runs_start_wait_attach`
- `service_smoke_blocks_existing_shell_supervisor_socket`
- `cargo test service_template_command_vectors_handle_quoted_paths --lib`
- `cargo test service_doctor_parses_systemd_installed_status --lib`
- `cargo test service_doctor_parses_launchd_installed_status --lib`
- `cargo test service_ --lib`
- `deepseek agents shell-fixture-smoke --json`
- `cargo test shell_supervisor_pty_fd_handoff_passes_master_fd --test shell_supervisor_owner_exit -- --nocapture`
- `cargo test agents_shell_fd_proxy_smoke_receives_native_pty_fd --test shell_supervisor_owner_exit -- --nocapture`
- `cargo test agents_shell_fd_proxy_abnormal_exit_releases_handoff --test shell_supervisor_owner_exit -- --nocapture`
- `cargo test agents_shell_fd_proxy_ctrl_c_interrupts_target_pty --test shell_supervisor_owner_exit -- --nocapture`
- `cargo test agents_shell_fd_proxy_forwards_sigwinch_resize --test shell_supervisor_owner_exit -- --nocapture`
- `cargo test agents_shell_fd_proxy_ctrl_d_eof_exits_cleanly --test shell_supervisor_owner_exit -- --nocapture`
- `cargo test service_smoke --lib`
- `cargo fmt --check`
- `cargo check`
- `cargo build --bin deepseek`
- `mkdir -p /tmp/dsc-smk`
- `target/debug/deepseek agents service-smoke --bin target/debug/deepseek --workdir /tmp/dsc-smk --json`
- `target/debug/deepseek agents service-smoke --kind systemd --installed --bin target/debug/deepseek --workdir /tmp/dsc-service-installed-gate-work --addr 127.0.0.1:8765 --json`
- Windows runner:
  `cargo test shell_supervisor_windows_tcp_daemon_client_smoke --lib -- --nocapture`
- `cargo test --lib -- --test-threads=1`
- `git diff --check`
