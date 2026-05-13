# DeepSeek-TUI Shell Log Replay Parity

Date: 2026-05-13

Status: implemented

## Gap

Background shell jobs persist stdout/stderr logs and `exec_shell_show` can
render a clipped snapshot. That is enough for inspection, but it is not enough
for a TUI or external client that wants deterministic replay without rereading
the entire log or losing bytes outside the snapshot clip.

## Spec

- Add a read-only `exec_shell_replay` tool.
- Address jobs by `task_id` plus optional `cwd`, using the same durable
  `.dscode/shell-jobs/<task_id>/` records as show/wait/list.
- Support `stream=stdout`, `stream=stderr`, and `stream=all`.
- Support byte `offset`, bounded `limit_bytes`, and `tail=true`.
- Return `offset`, `next_offset`, and `total_bytes` so clients can continue
  replaying from the exact next byte.
- Work after the original DeepSeekCode process exits, as long as durable logs
  and manifests remain.
- Expose the tool in the registry, model tool specs, and MCP read-only surface.

## Implementation

- Added `ExecShellReplayTool` in `src/tools/exec_shell.rs`.
- Added bounded durable log-slice rendering over `stdout.log` and `stderr.log`.
- Reused durable manifest loading and stale-running status refresh before
  rendering replay slices.
- Registered `exec_shell_replay` in the default tool registry.
- Added static model schema and MCP tool definition.
- Kept side-effect policy read-only; replay does not start, write to, or cancel
  shell jobs.

## Verification

- `/home/willamhou/.cargo/bin/cargo test exec_shell_replay_reads_durable_log_offsets --lib`
- `/home/willamhou/.cargo/bin/cargo test exec_shell --lib`
- `/home/willamhou/.cargo/bin/cargo test default_registry_includes_exec_shell_background_tools --lib`
- `/home/willamhou/.cargo/bin/cargo test build_tool_specs_include_exec_shell_background_tools --lib`
- `/home/willamhou/.cargo/bin/cargo test serve --lib`
- `/home/willamhou/.cargo/bin/cargo fmt --check`
- `/home/willamhou/.cargo/bin/cargo check`
- `git diff --check`

## Remaining Gap

This is durable log replay, not terminal replay. It does not preserve alternate
screen state, cursor movements, resize events, input timing, or an attachable
PTY session. Those still require a real PTY supervisor and terminal event log.
