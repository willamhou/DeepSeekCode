# DeepSeek-TUI Shell Supervisor PTY Design

Date: 2026-05-13

Status: designed

## Gap

DeepSeekCode now supports DeepSeek-TUI-compatible shell names, durable shell
manifests/logs, detached status refresh, detached cancel, Unix FIFO stdin,
`tty=true` execution through `script`, initial PTY geometry, durable log replay,
owner/process-group metadata, Linux native-supervisor PTY ownership, and a local
SCM_RIGHTS `pty_fd` handoff slice. The remaining shell gap is no longer another
manifest field. It is hardening that supervisor path across lifecycle edges and
proving equivalent behavior on other platforms.

The current `script` backend is useful for commands that require a TTY, but it
does not expose the PTY master fd to DeepSeekCode. That means DeepSeekCode
cannot honestly implement live resize, raw terminal attach, cursor-preserving
terminal replay, or authoritative PTY lifecycle ownership on top of the current
backend alone.

## Success Criteria

- A supervised shell session survives the original DeepSeekCode CLI process
  exiting.
- A supervisor-owned native PTY backend records enough terminal events for
  deterministic attach/replay from a byte or event cursor.
- Resize control updates the live PTY window size and signals the child process.
- Existing tool names remain compatible: `exec_shell`, `task_shell_start`,
  `exec_shell_wait`, `exec_shell_show`, `exec_shell_replay`,
  `exec_shell_interact`, and `exec_shell_cancel`.
- New attach/resize operations are explicit and fail with clear diagnostics
  when the active backend is still `script` or plain pipes.
- Side-effect policy remains unchanged: shell start, stdin, resize, attach-live,
  and cancel require the same trusted execution or durable approval gates as
  current mutating shell tools.

## Proposed Architecture

### Process Model

- Add a workspace-local shell supervisor process, launched by TUI, `serve`, or
  `agents service` when supervised PTY sessions are requested.
- Store supervisor state under `.dscode/shell-supervisor/`.
- Keep per-job durable records under the existing
  `.dscode/shell-jobs/<task_id>/` layout so current detached show/wait/list
  code has a stable migration path.
- The supervisor, not the initial CLI, owns:
  - native PTY master fd
  - child process group
  - terminal event writer
  - stdin/control socket
  - resize handling

### Manifest Additions

For supervisor-backed sessions, extend `manifest.json` with nullable fields:

- `supervisor_pid`
- `supervisor_socket`
- `supervisor_epoch`
- `pty_backend: "native-supervisor"`
- `terminal_event_log`
- `terminal_event_seq`
- `control_token_hash`
- `attachable: true`
- `resizable: true`

Older records remain valid. Current `script` records continue to render
`attachable: false` and `resizable: false` if these fields are absent.

### Control Protocol

Use a Unix domain socket on Unix:

- socket path: `.dscode/shell-supervisor/supervisor.sock`
- permissions: `0600`
- request authentication: per-workspace random control token, stored hashed in
  supervisor state and never printed in normal tool output
- request/response encoding: newline-delimited JSON

Initial methods:

- `start`
- `show`
- `wait`
- `replay`
- `attach`
- `stdin`
- `resize`
- `cancel`
- `shutdown`

Windows native PTY support is now a ConPTY-specific backend slice using
`portable-pty` for supervisor-started `native-supervisor` jobs. The current
Windows implementation covers start, output event logging, stdin, wait,
terminal replay/attach, and resize through the same in-process protocol helpers.
The long-running daemon/client IPC now has a Windows first slice: the supervisor
binds a loopback TCP listener, writes `tcp://127.0.0.1:<port>` to
`.dscode/shell-supervisor/supervisor.tcp`, stores the same endpoint in the
manifest, and reuses the newline-delimited JSON protocol. Windows
`attach_stream` can use that stream path. `byte_stream`/`raw_proxy` and
Linux-only `pty_fd` remain unsupported on Windows; named pipe IPC remains a
future option if loopback TCP is not sufficient for installed service use.

### Terminal Event Log

Add `.dscode/shell-jobs/<task_id>/terminal-events.jsonl` for supervisor-backed
PTY sessions. Each event has a monotonic `seq`, timestamp, and kind:

- `started`
- `output`
- `input`
- `resize`
- `status`
- `exit`
- `cancelled`

Native-supervisor `output` and `input` payloads now store optional raw PTY bytes
as `raw_base64` plus a display-safe preview. `exec_shell_replay` can keep its
current stdout/stderr byte mode for old jobs and add `stream=terminal` for
supervisor jobs.

### Attach Contract

Attach is an API-level terminal stream, not a full UI widget:

- `exec_shell_attach task_id=<id> cursor=<seq>` replays terminal events from the
  cursor and returns `next_cursor`.
- In MCP/ACP or HTTP modes, attach can optionally keep the request open and
  stream event frames.
- In local TUI mode, the TUI consumes the same event stream and renders an
  attachable terminal pane.

### Resize Contract

`exec_shell_resize task_id=<id> tty_rows=<n> tty_cols=<n>`:

- requires a running supervisor-backed PTY session
- updates the PTY window size with `TIOCSWINSZ`
- sends `SIGWINCH` to the child process group on Unix
- persists a `resize` terminal event
- updates manifest `tty_rows` and `tty_cols`
- returns a clear diagnostic for non-PTY, completed, stale, detached-old, and
  `script` backend jobs

## Implementation Slices

1. Supervisor protocol skeleton:
   - workspace-local socket
   - health/show methods
   - manifest fields
   - status: landed
2. Native Unix PTY backend:
   - Linux `posix_openpt`/`setsid`/`TIOCSCTTY` FFI
   - supervisor-owned master fd for `deepseek agents shell-supervisor`
     `tty=true` starts
   - child process group
   - terminal event log writer
   - status: first Linux slice landed
3. Replay and attach:
   - `stream=terminal`
   - event cursor support
   - native-supervisor output/input event records persist `raw_base64`
   - supervisor `attach` JSON responses include structured
     `terminal_raw_outputs` for output-event bytes
   - supervisor `attach_stream` emits newline-JSON attach frames on one Unix
     socket connection for follow clients
   - supervisor `byte_stream` emits newline-JSON raw-output byte frames on one
     Unix socket connection, with optional initial stdin/resize control and
     later in-stream stdin/resize/close/detach control frames
   - supervisor `byte_stream raw_proxy=true` switches the same socket to raw
     bytes after the initial JSON request, forwarding socket bytes to PTY stdin
     and writing decoded PTY output bytes back without JSON framing
   - supervisor `pty_fd` on Linux sends a duplicated native-supervisor PTY
     master fd to a local Unix client with SCM_RIGHTS, pauses the replay reader
     during the lease, records `fd_handoff` lifecycle events, and resumes
     ordinary supervisor stdin/resize/replay control after the client detaches
   - `exec_shell_attach` includes output-event `terminal_raw_base64` as a
     compatible text-summary section; human `deepseek agents shell attach
     --follow` and `--interactive` decode raw output bytes before falling back
     to text summaries
   - `deepseek agents shell attach --raw` emits decoded PTY output bytes for
     one-shot or follow-mode script consumers
   - `deepseek agents shell proxy <task_id>` wraps `byte_stream raw_proxy=true`
     for human operators: local raw mode, initial terminal-size sync, key/paste
     byte forwarding, resize forwarding through supervisor `resize`, direct PTY
     byte output, and `Ctrl-]` detach
   - `deepseek agents shell fd-proxy <task_id>` wraps Linux `pty_fd` handoff for
     local raw-mode PTY takeover through the received master fd; Ctrl-D EOF is
     covered, Linux PTY master `EIO` after slave close exits cleanly, and
     local SIGWINCH resize updates the handed-off PTY size; Ctrl-C is covered
     as target PTY SIGINT delivery; killed-client lease release is covered
   - MCP/ACP schema exposure landed through ACP `session/shell/subscribe` and
     MCP `exec_shell_terminal_events`; HTTP SSE emits `raw_base64`, while ACP
     and MCP progress metadata emit `rawBase64`
4. Resize:
   - `exec_shell_resize`
   - `TIOCSWINSZ`
   - `SIGWINCH`
   - persisted `resize` terminal event
   - tests verify the native supervisor resize path, event log, and an
     end-to-end `stty size` assertion inside the child PTY after resize
5. Owner-exit integration:
   - integration test that starts the real shell-supervisor daemon, starts a
     supervised native PTY job through one socket connection, drops that
     connection, then replays/resizes/attaches/cancels through fresh socket
     connections
   - detached tool-level stdin/resize/cancel now forward through
     `supervisor_socket` for running native-supervisor manifests
   - status: daemon/socket owner-exit smoke, detached tool forwarding, and
     child-observed resize verification landed
6. Human CLI wrapper:
   - `deepseek agents shell ...` forwards status/show/start/wait/replay/attach/
     stdin/resize/cancel/shutdown requests to the workspace supervisor socket
   - non-JSON mode prints relevant tool summaries; `--json` prints raw protocol
     responses
   - status: first slice landed
7. Service packaging:
   - systemd/launchd templates can supervise the shell supervisor alongside
     runtime and diagnostics services
8. Windows ConPTY:
   - `native-supervisor` can now spawn Windows ConPTY jobs through
     `portable-pty`
   - shell-supervisor daemon/client IPC can now use a workspace-local loopback
     TCP endpoint recorded in `.dscode/shell-supervisor/supervisor.tcp`
   - Windows target compile gate passes with
     `cargo check --target x86_64-pc-windows-gnu --all-targets`
   - CI has targeted Windows endpoint/status, TCP daemon/client, real binary
     shell-fixture, and start/resize smoke commands; the actual runner evidence
     still needs to be collected

## Verification Plan

Future implementation should add these gates:

- `cargo test exec_shell_supervisor_protocol --lib`
- `cargo test exec_shell_supervisor_replay_terminal_events --lib`
- `cargo test exec_shell_supervisor_resize_updates_tty_size --lib`
- `cargo test exec_shell_replay_reads_terminal_event_log_by_cursor --lib`
- `cargo test agents_shell --lib`
- `cargo test agents_shell_cli_args_build_protocol_requests --lib`
- `cargo test agents_shell_cli_controls_supervised_native_pty --test shell_supervisor_owner_exit`
- `cargo test terminal_event_log_records_raw_base64_for_byte_replay --lib`
- `cargo test mcp_tools_call_shell_terminal_events_emits_progress_notifications --lib`
- `cargo test acp_session_shell_subscribe_pushes_terminal_events --lib`
- `cargo test shell_terminal_event_stream_endpoint_replays_sse_frames --lib`
- `cargo test --test shell_supervisor_owner_exit`
- `cargo check --target x86_64-pc-windows-gnu --all-targets`
- Windows runner:
  `cargo test exec_shell_supervisor_status_treats_tcp_endpoint_as_ready --lib -- --nocapture`
- Windows runner:
  `cargo test shell_supervisor_tcp_endpoint_parser_accepts_loopback_only --lib -- --nocapture`
- Windows runner:
  `cargo test shell_supervisor_windows_tcp_daemon_client_smoke --lib -- --nocapture`
- Windows runner:
  `cargo test shell_supervisor_protocol_tty_start_records_native_pty_events --lib -- --nocapture`
- Windows runner:
  `cargo test shell_supervisor_protocol_native_pty_resize_records_event --lib -- --nocapture`
- Windows runner:
  `deepseek agents shell-fixture-smoke --json`
- `cargo test serve --lib`
- `cargo fmt --check`
- `cargo check`
- `git diff --check`

## Current Decision

Do not add fake live resize or fake attach on top of the `script` backend.
The supervisor protocol skeleton, terminal event replay/attach plumbing, the
first Linux `native-supervisor` PTY backend, and the Windows ConPTY
`native-supervisor` backend have landed. Normal
`exec_shell tty=true` still uses `script`; shell-supervisor `tty=true` starts
own a native PTY master, write `terminal-events.jsonl`, and support live
Linux `TIOCSWINSZ` or Windows ConPTY resize through the in-process supervisor.
The Windows daemon/client path now has a loopback TCP endpoint first slice using
the same newline JSON protocol. CI now wires TCP daemon/client and real binary
shell-fixture smoke; the actual Windows runner result is still needed before
closing platform parity.
HTTP shell terminal SSE,
ACP `session/shell/subscribe`, and MCP `exec_shell_terminal_events` progress
notifications now cover protocol-level terminal event consumption, including
optional base64 raw PTY bytes for output/input events. Human follow and
interactive attach now consume those raw output bytes when available, and
`--raw` exposes them directly for scripts. Supervisor `attach` now exposes a
structured `terminal_raw_outputs` field, `attach_stream` provides repeated
attach frames over one socket connection for follow clients, and `byte_stream`
adds duplex stdin/resize control frames plus raw-output byte frames for
scriptable PTY consumers. `raw_proxy=true` now provides a raw socket-byte proxy
slice over the same event-log attach path, `deepseek agents shell proxy` adds a
human raw-mode wrapper around it, and Linux `pty_fd` hands the native PTY master
fd to a local Unix client with SCM_RIGHTS. Ctrl-C, Ctrl-D EOF, SIGWINCH resize,
and killed-client release now have coverage through the CLI fd-proxy path.
Remaining hard slices are actual installed systemd/launchd service smoke
evidence and the actual Windows CI runner result for the wired ConPTY/TCP
shell-supervisor gates.
