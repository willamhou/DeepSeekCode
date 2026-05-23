# Custom Subagents

Custom subagents are Markdown files with YAML frontmatter. Project agents live in
`.dscode/agents/*.md`; user agents live in `~/.config/dscode/agents/*.md`.

## File Format

```md
---
name: reviewer
description: Reviews code and points out concrete risks
tools: [read_file, search_text, git_diff]
model: deepseek-coder
---
Review the assigned scope. Prioritize correctness bugs, regressions, and missing
tests. Return concise findings with file paths and line numbers when possible.
```

Supported frontmatter fields:

- `name`: stable agent name, using letters, numbers, `_`, `-`, or `.`
- `description`: one-line summary shown in `deepseek agents list`
- `tools`: optional advisory tool list for the child task prompt
- `model`: optional advisory model label

The Markdown body is the subagent prompt.

## CLI Management

List configured agents:

```sh
deepseek agents list
```

Show one agent:

```sh
deepseek agents show reviewer
```

Validate all configured agent files:

```sh
deepseek agents validate
```

Validate one file:

```sh
deepseek agents validate .dscode/agents/reviewer.md
```

Run a pending durable runtime task that is linked to a runtime thread:

```sh
deepseek agents run-task task-...
deepseek agents run-task --budget 8 --json task-...
```

`run-task` claims the task, executes the local agent loop in the thread
workspace, appends user/assistant turns, tool results, usage, and task status
back to `.dscode/runtime`, and creates a pre-run rollback snapshot when the
workspace is a git worktree. Permissioned write/shell/MCP tool calls append a
durable `permission_request` event and wait for a matching
`permission_response`, so a TUI or HTTP runtime client can approve or deny a
background daemon task.

Run a local runtime daemon that polls pending tasks and due automations:

```sh
deepseek agents daemon
deepseek agents daemon --interval-ms 1000 --budget 8
deepseek agents daemon --once --json
```

`daemon` triggers active automations whose `next_run_at` is due, supports
recurring schedules such as `every:60s`, `every:5m`, and `@every 1h`, executes
one thread-linked pending task per tick through the same durable `run-task`
path, recovers stale live RLM ownership, and runs one queued live RLM turn per
tick.

## Background Worktree Tasks

For long local tasks that should not block the parent CLI process, use the
repo-local worktree runner:

```sh
deepseek task start "fix the failing tests and summarize the diff"
deepseek task list
deepseek task show task-...
deepseek task diff task-... --stat
deepseek task merge task-... --check
deepseek task merge task-...
deepseek task reject task-...
deepseek task stop task-...
```

`task start` creates an isolated git worktree under
`.dscode/task-runner/worktrees/<id>`, creates a branch named
`deepseek-task/<id>` by default, writes a durable record under
`.dscode/task-runner/records/`, writes stdout/stderr logs under
`.dscode/task-runner/logs/`, and launches `deepseek exec --json` in that
worktree. The parent command can exit while the child process continues.

Use `--no-run` to create only the worktree and record without spending model
calls. `task diff` shows the worktree patch/stat plus untracked files.
`task merge` applies the task worktree diff back to the original repo after a
clean-worktree check, with `--check` for dry-run verification. `task reject`
marks the record rejected and removes the managed task worktree by default.
`task fixture-smoke --json` is the local no-credential gate for create, merge,
and reject behavior.

Render local supervisor files for running the HTTP runtime, the durable task
and live RLM worker daemon, diagnostics watch, and the shell supervisor
protocol bridge as long-lived services:

```sh
deepseek agents service --kind systemd --out ./services --workdir "$PWD" --bin "$(command -v deepseek)"
deepseek agents service --kind launchd --out ./services --workdir "$PWD" --bin "$(command -v deepseek)"
```

The generated files are reviewable templates; the command does not enable or
start services. Static placeholder templates also live under
`packaging/systemd/` and `packaging/launchd/`, while release packages include
them under `services/`.

Before installing those files, run the static service doctor against the same
binary/workspace/template directory:

```sh
deepseek agents service-doctor --kind all --out ./services --workdir "$PWD" --bin "$(command -v deepseek)"
deepseek agents service-doctor --kind all --out ./services --workdir "$PWD" --bin "$(command -v deepseek)" --json
```

`service-doctor` does not start systemd or launchd services. It verifies the
selected binary and workspace, checks that the rendered runtime, agents,
diagnostics, and shell-supervisor templates contain the expected command
topology, parses systemd `ExecStart` plus `WorkingDirectory` and launchd
`ProgramArguments` plus `WorkingDirectory` back into exact argv/workdir vectors,
and reports stale or missing files under `--out` as blockers. The command-vector
check catches quoting or argument-order regressions before the templates are
installed. Missing platform service-manager commands such as `systemctl` or
`launchctl` are warnings so the same check can run in CI containers.

After copying and enabling the generated files, add `--installed` to make the
same doctor fail closed on the actual user services:

```sh
deepseek agents service-doctor --kind systemd --installed --workdir "$PWD" --bin "$(command -v deepseek)" --json
deepseek agents service-doctor --kind launchd --installed --workdir "$PWD" --bin "$(command -v deepseek)" --json
```

The installed mode is read-only: it does not install, enable, restart, or stop
services. It inspects `systemctl --user show` for the four systemd units or
`launchctl print gui/<uid>/<label>` for the four launchd labels and reports
missing, inactive, failed, or disabled services as blockers.

To prove the selected binary can actually start the two core long-lived local
surfaces without installing a service manager, run:

```sh
mkdir -p /tmp/dsc-smk
deepseek agents service-smoke --workdir /tmp/dsc-smk --bin "$(command -v deepseek)"
deepseek agents service-smoke --workdir /tmp/dsc-smk --bin "$(command -v deepseek)" --json
```

`service-smoke` starts `serve --http --once` on a loopback ephemeral port,
probes `/health`, waits for that child to exit, then starts
`agents shell-supervisor --json`, probes the Unix socket `health` method, runs
`start` -> `wait` -> `attach` -> `replay` against a small command, requests
`shutdown`, and waits for the supervisor child to exit. On Linux, that control
smoke requests `tty=true`, requires the `native-supervisor` PTY backend, and
also verifies PTY `stdin`, `resize`, terminal `replay`, and `cancel`; on other
Unix platforms it uses non-TTY shell execution. On non-Unix platforms, the
shell-supervisor portion is reported as a warning because the protocol socket
is Unix-only. If the target workspace already has a live shell-supervisor
socket, the smoke check reports a blocker and does not send `shutdown` to the
existing process; use a short isolated `--workdir` such as `/tmp/dsc-smk` for
release evidence because Unix socket paths have platform length limits.
On Linux, the same control smoke also exercises supervisor `byte_stream`
duplex control frames, `raw_proxy=true` raw socket bytes, `pty_fd` fd handoff,
and the human `deepseek agents shell proxy` raw-mode wrapper. Focused Linux
tests also verify that after direct `pty_fd` or CLI `fd-proxy` detaches, normal
supervisor `resize`, `stdin`, and terminal `replay` continue on the same PTY
job; CLI `fd-proxy` also treats Linux PTY `EIO` after slave close as normal EOF
and has Ctrl-C interrupt, Ctrl-D EOF, SIGWINCH resize, and killed-client
release coverage.

After installing and enabling the generated user services, run installed smoke
with a concrete runtime address:

```sh
deepseek agents service-smoke --kind systemd --installed --addr 127.0.0.1:8765 --workdir "$PWD" --bin "$(command -v deepseek)" --json
deepseek agents service-smoke --kind launchd --installed --addr 127.0.0.1:8765 --workdir "$PWD" --bin "$(command -v deepseek)" --json
```

Installed smoke is also read-only. It first reuses the installed service checks,
then probes the actual runtime `/health` endpoint and the existing workspace
shell-supervisor endpoint: Unix uses `.dscode/shell-supervisor/supervisor.sock`,
and Windows uses `.dscode/shell-supervisor/supervisor.tcp`. It runs
shell-supervisor control checks without sending `shutdown`, so it does not stop
a service-manager-owned supervisor. Unlike the non-installed smoke,
`--installed` rejects `127.0.0.1:0`; use the concrete address configured in the
installed service template.

For a focused local Shell/PTY gate that starts only an isolated supervisor
fixture, run:

```sh
deepseek agents shell-fixture-smoke --json
```

This command creates a temporary workspace, starts the current binary as
`agents shell-supervisor --json`, probes `health`, runs start/wait/attach/replay
control checks, and on Linux validates native PTY stdin/resize/replay/cancel
plus `byte_stream` duplex frames, raw proxy output, `pty_fd` fd handoff, and the
human `agents shell proxy` wrapper. It is intended as the fast local evidence
command for shell-supervisor parity work.

The shell supervisor service runs `deepseek agents shell-supervisor --json`.
It binds the workspace-local `.dscode/shell-supervisor/supervisor.sock` and
writes a manifest for `exec_shell_supervisor_status`; where the platform
supports it, supervisor-owned native PTY jobs advertise attach/stdin/resize/
replay/wait/cancel protocol capabilities through the same manifest.
For human terminal monitoring, `deepseek agents shell attach <task_id>
--follow` uses the supervisor `attach_stream` method to print new terminal
payloads from newline-delimited JSON frames on one Unix socket connection; use
`--max-ms` for a bounded follow in scripts. Add `--raw` to emit only decoded
PTY output bytes from supervisor `terminal_raw_outputs` instead of human
summaries. `deepseek agents shell attach <task_id> --interactive` /
`--takeover` enters local raw mode, forwards keys through supervisor `stdin`,
forwards terminal resize metadata, and replays terminal output bytes back to
the operator until the job exits, `--max-ms` expires, or `Ctrl-]` detaches.
For scriptable byte-stream control, `deepseek agents shell byte-stream
<task_id> --input 'cmd\n' --rows 34 --cols 102 --json` uses one supervisor
socket request to apply optional stdin/resize control and then emits
newline-JSON frames with `byte_outputs[].bytes_base64`. The same
`byte_stream` socket also accepts later newline-JSON control frames such as
`{"type":"stdin","input":"..."}` and `{"type":"resize","rows":34,"cols":102}`;
applied frames are echoed in `control_frames`. Add `--raw-proxy` to switch the
same socket from JSON frames to raw PTY proxy mode after the initial request:
socket input bytes are forwarded to PTY stdin and PTY output bytes are written
back without JSON framing.
For human terminal takeover, `deepseek agents shell proxy <task_id>` wraps the
same raw-proxy stream in local raw mode, syncs the current terminal size,
forwards key/paste bytes and resize events, prints PTY output bytes directly,
and detaches with `Ctrl-]`.
On Linux native-supervisor jobs, `deepseek agents shell fd-proxy <task_id>`
requests supervisor `pty_fd`, receives the PTY master fd over the Unix socket
with SCM_RIGHTS, temporarily pauses the supervisor replay reader for exclusive
local PTY access, forwards local terminal input/output directly through that fd,
and detaches with `Ctrl-]`. On detach, the supervisor records the handoff end,
resumes its replay reader, and can again accept ordinary `resize`/`stdin`
control for that job. When the target PTY closes, Linux `EIO` from the master
fd is treated as EOF so Ctrl-D-driven shell exits close cleanly. This is local
Unix fd handoff only. Local terminal `SIGWINCH` resize events are forwarded to
the handed-off PTY master so the child PTY observes the new rows/cols. Ctrl-C
is forwarded as the terminal interrupt byte and reaches the target PTY
foreground process group. If the local fd-proxy client is killed, the closed
control socket releases the handoff lease and normal supervisor control resumes.
Windows now has a `native-supervisor` ConPTY backend behind the same in-process
protocol helpers and Windows target compile gate. The Windows daemon/client
control path now has a loopback TCP first slice stored at
`.dscode/shell-supervisor/supervisor.tcp` and uses the same newline JSON
protocol for health/status/show/start/wait/replay/attach/attach_stream/stdin/
resize/cancel/shutdown. Windows `byte_stream/raw_proxy` and `pty_fd` remain
unsupported. CI now wires a Windows TCP daemon/client unit smoke, a real binary
`agents shell-fixture-smoke --json`, and targeted ConPTY start/resize smoke; the
remaining Windows parity proof is the actual runner result. Installed service
proof remains separate parity work.

After a supervisor starts the agents daemon, use the RLM lifecycle commands to
check live worker state:

```sh
deepseek agents rlm-status --json
deepseek agents rlm-events <session_id> --cursor 0 --json
deepseek agents rlm-wait <session_id> --cursor 0 --timeout-ms 5000 --json
```

List subagent thread artifacts created by parallel dispatch:

```sh
deepseek agents threads
```

Run the local no-network subagent gate:

```sh
deepseek agents subagent-fixture-smoke --json
```

The fixture validates the parallel request parser, disjoint `write_scope`
metadata, parent readback metadata, blocker summaries, write-scope conflict
summaries, and `.dscode/agent-threads/*.md` artifact shape.

Show or switch the active thread:

```sh
deepseek agents show-thread thread-...
deepseek agents switch thread-...
deepseek agents current
deepseek agents clear-current
```

## Dispatch

`dispatch_subagent` accepts an optional `agent` argument. When set, DeepseekCode
loads the matching project or user agent and injects its prompt into the child
task. Project agents take precedence over user agents with the same name.

`dispatch_subagents` accepts a `tasks` JSON array and runs up to 4 child
subagents concurrently. Each task object requires `task` and may include
`agent`, `skill`, `write_scope`, and `steps`:

```json
[
  {"task": "Review src/api.rs for correctness risks", "agent": "reviewer", "write_scope": "src/api.rs", "steps": "4"},
  {"task": "Inspect docs/install.md for release gaps", "write_scope": "docs/install.md", "steps": "3"}
]
```

The consolidated result includes per-child thread IDs, outcomes, next actions,
write scopes, readback files, blocker counts, write-scope conflict summaries,
and `.dscode/agent-threads/*.md` artifacts. Use `deepseek agents switch <id>`
to mark the thread you want to inspect or continue from. When child summaries
include edited or inspected files, the parent planner treats the result as
requiring readback before relying on the child patch.
