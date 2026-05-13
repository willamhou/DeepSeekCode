# DeepSeek-TUI Shell Owner Metadata

Date: 2026-05-13

Status: implemented

## Gap

Durable shell manifests recorded a child `pid`, but completed attached jobs
could lose the original child pid once the in-memory `Child` handle was gone.
The manifest also did not record which DeepSeekCode process created the job or
which Unix process group detached cancellation targets. That made detached
snapshots less clear than a supervisor-style shell surface.

## Spec

- Persist a stable child `pid` for the life of the shell record.
- Persist the owning DeepSeekCode process id as `owner_pid`.
- Persist the Unix process group target as `process_group`.
- Render `owner_pid`, `owner_alive`, child `pid`, and `process_group` in
  wait/show snapshots.
- Preserve detached compatibility for older manifests where owner metadata is
  absent.
- Keep this diagnostic only; do not claim full supervisor ownership.

## Implementation

- Added `owner_pid`, `child_pid`, and `process_group` to in-memory background
  shell jobs.
- Changed manifest persistence to write the stored child pid even after the
  child handle has been waited and removed.
- Added nullable `owner_pid` / `process_group` support to durable manifest
  read/write paths.
- Changed detached cancel to target the persisted `process_group` when present,
  with fallback to older manifests that only recorded `pid`.
- Added owner liveness rendering with `owner_alive`.
- Added a regression test covering stable child pid after completion.

## Verification

- `/home/willamhou/.cargo/bin/cargo test exec_shell_manifest_keeps_child_pid_after_completion --lib`
- `/home/willamhou/.cargo/bin/cargo test exec_shell --lib`
- `/home/willamhou/.cargo/bin/cargo test serve --lib`
- `/home/willamhou/.cargo/bin/cargo fmt --check`
- `/home/willamhou/.cargo/bin/cargo check`
- `git diff --check`

## Remaining Gap

This improves supervisor-style introspection, but ownership is still best
effort. There is no independent shell supervisor daemon that owns PTY lifecycle,
survives the CLI owner exiting, reports authoritative terminal state, or
performs live resize/attach operations.
