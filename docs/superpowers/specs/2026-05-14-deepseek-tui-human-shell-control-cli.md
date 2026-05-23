# DeepSeek-TUI Human Shell Control CLI

Date: 2026-05-14

Status: second slice implemented

## Context

DeepSeekCode already had shell-supervisor protocol methods and tool-level
control for durable shell jobs, including Linux native PTY jobs. The remaining
usability gap was a human-facing CLI entry point for those protocol controls.

## Implementation

- Added `deepseek agents shell ...` as a thin CLI wrapper around the
  workspace-local shell supervisor socket.
- Supported actions:
  - `status`
  - `show`
  - `start`
  - `wait`
  - `replay`
  - `attach`
  - `stdin` / `send`
  - `resize`
  - `cancel`
  - `shutdown`
- `start` accepts `--tty`, `--cwd`, `--rows`, `--cols`, `--json`, and command
  arguments after `--`.
- `resize` accepts `--rows` / `--cols` or positional `rows cols`.
- `stdin` accepts `--input`, `--close-stdin`, and `--timeout-ms`.
- `attach` accepts `--follow`, `--poll-ms`, and `--max-ms` for a human-facing
  terminal follow loop that prints only new terminal payloads while the job is
  still running. Follow mode uses supervisor `attach_stream` newline-JSON
  frames over one Unix socket connection instead of repeatedly opening new
  attach requests.
- `attach` accepts `--raw` for one-shot or follow-mode script consumers that
  want decoded PTY output bytes instead of human terminal summaries.
- `byte-stream` accepts `--input`, `--close-stdin`, `--rows`, `--cols`,
  `--cursor`, `--wait-ms`, `--poll-ms`, `--max-ms`, `--max-events`,
  `--limit-bytes`, `--tail`, and `--json`. It uses supervisor `byte_stream`
  to apply optional stdin/resize control and then stream raw PTY output bytes
  on the same Unix socket request. Non-JSON mode writes decoded bytes directly;
  JSON mode prints newline-delimited frames with `byte_outputs[].bytes_base64`.
  The same protocol stream now drains later newline-JSON control frames for
  `stdin`, UTF-8 `bytes_base64` stdin, `resize`, `close_stdin`, and `detach`;
  successful control frames are echoed in `control_frames`. `--raw-proxy`
  switches to raw socket bytes after the initial JSON request, forwarding
  socket input bytes into PTY stdin and returning PTY output bytes without JSON
  framing.
- `proxy` / `pty-proxy` is now the human-facing wrapper over `byte_stream
  raw_proxy=true`: it enters local raw mode, syncs the current terminal size,
  forwards key and paste bytes over the raw proxy, forwards resize events
  through supervisor `resize`, writes PTY output bytes directly to the local
  terminal, and detaches with `Ctrl-]`.
- `fd-proxy` / `pty-fd` is the Linux native-supervisor fd handoff wrapper:
  it requests supervisor `pty_fd`, receives the duplicated PTY master fd over
  SCM_RIGHTS, uses that fd for local raw-mode terminal takeover, and detaches
  with `Ctrl-]`. During the lease the supervisor pauses its replay reader and
  records `fd_handoff` lifecycle events; after detach it resumes ordinary
  supervisor stdin/resize/replay control for the same PTY job. Linux PTY master
  `EIO` after slave close is treated as EOF so Ctrl-D-driven shell exits return
  success. Local terminal `SIGWINCH` resize events are applied to the
  handed-off PTY master. Ctrl-C is forwarded to the target PTY foreground
  process group and is covered by an end-to-end SIGINT smoke. A killed client
  closes the control socket, releases the supervisor fd lease, and restores
  normal supervisor stdin/resize/replay control.
- `attach` also accepts `--interactive` / `--takeover` for a bounded terminal
  control loop. The local terminal enters raw mode, keyboard events are mapped
  to terminal input bytes and forwarded through supervisor `stdin`, terminal
  resize events are forwarded through supervisor `resize`, and stdout replay is
  streamed back to the local terminal until the job exits or the operator
  detaches with `Ctrl-]`.
- Non-JSON output prints the relevant supervisor summary; `--json` prints the
  raw protocol response. In `--follow --json` mode, each `attach_stream` frame
  is printed as newline-delimited protocol JSON.
- Shell completions now include both `shell` and `shell-supervisor` under
  `agents`.
- Release service documentation now points operators to `deepseek agents shell
  ...` for human protocol control.

## Verification

- `cargo test cli_from_argv_routes_agents_subcommands --lib`
- `cargo test agents_shell_cli_args_build_protocol_requests --lib`
- `cargo test agents_shell_cli_controls_supervised_native_pty --test shell_supervisor_owner_exit -- --nocapture`
- `cargo test agents_shell_attach_follow_parses_cursor_status_and_payload --lib`
- `cargo test agents_shell_attach_interactive_maps_terminal_keys --lib`
- `cargo test agents_shell_attach_interactive_smoke_forwards_input_and_detaches --test shell_supervisor_owner_exit -- --nocapture`
- `cargo test agents_shell_proxy_smoke_uses_raw_proxy_and_terminal_size --test shell_supervisor_owner_exit -- --nocapture`
- `cargo test shell_supervisor_pty_fd_handoff_passes_master_fd --test shell_supervisor_owner_exit -- --nocapture`
- `cargo test agents_shell_fd_proxy_smoke_receives_native_pty_fd --test shell_supervisor_owner_exit -- --nocapture`
- `cargo test agents_shell_fd_proxy_abnormal_exit_releases_handoff --test shell_supervisor_owner_exit -- --nocapture`
- `cargo test agents_shell_fd_proxy_ctrl_c_interrupts_target_pty --test shell_supervisor_owner_exit -- --nocapture`
- `cargo test agents_shell_fd_proxy_forwards_sigwinch_resize --test shell_supervisor_owner_exit -- --nocapture`
- `cargo test agents_shell_fd_proxy_ctrl_d_eof_exits_cleanly --test shell_supervisor_owner_exit -- --nocapture`
- `deepseek agents shell-fixture-smoke --json` now includes the human proxy
  wrapper and `pty_fd` fd handoff in the Linux shell control smoke.
- `cargo test shell_supervisor_protocol --lib`
- `cargo test exec_shell_replay_reads_terminal_event_log_by_cursor --lib`
- `cargo test terminal_event_log_records_raw_base64_for_byte_replay --lib`
- `cargo test mcp_tools_call_shell_terminal_events_emits_progress_notifications --lib`
- `cargo test acp_session_shell_subscribe_pushes_terminal_events --lib`
- `cargo test shell_terminal_event_stream_endpoint_replays_sse_frames --lib`
- `cargo fmt --check`
- `cargo check --all-targets`
- `git diff --check`

## Residual

`--follow` is still a bounded cursor-following `attach_stream` over durable
attach snapshots, and `--interactive` is a bounded raw-key/raw-output replay
control loop over supervisor protocol methods. `byte-stream` is a duplex byte
proxy slice because it combines initial and in-stream stdin/resize control
frames with raw-output frames. `--raw-proxy` removes the JSON frame layer for
the data path. Linux `fd-proxy` is now the actual local PTY master fd handoff
slice, but it is not yet cross-platform and does not replace the need for
Windows ConPTY or installed service evidence. Direct `pty_fd` and CLI
`fd-proxy` regression tests now cover release back to normal supervisor
stdin/resize/replay after detach, CLI Ctrl-C interrupt, Ctrl-D EOF clean exit,
SIGWINCH resize forwarding, and killed-client lease release.
Both modes now prefer output bytes decoded from `terminal_raw_base64` when a
native-supervisor terminal event log has raw PTY output records. Supervisor
`attach` responses now expose structured `terminal_raw_outputs`, and the human
CLI uses that before falling back to the compatible summary section.
`attach_stream` keeps follow mode on one supervisor socket connection, and
`byte_stream` keeps scriptable stdin/resize plus raw-output frames on one
supervisor socket request, and `--raw-proxy` exposes the same stream as raw
socket bytes after the initial JSON request. `deepseek agents shell proxy`
adds a local raw terminal wrapper around that raw-proxy path; `deepseek agents
shell fd-proxy` adds local Linux SCM_RIGHTS PTY master fd handoff.
`--raw` exposes decoded attach bytes without summary text for attach scripts.
Remaining shell-supervisor parity work is actual installed systemd/launchd
service smoke evidence and Windows ConPTY. HTTP shell terminal SSE, ACP
`session/shell/subscribe`, and MCP
`exec_shell_terminal_events` progress notifications now cover protocol
terminal event consumption, including optional base64 raw PTY bytes for
output/input records.
