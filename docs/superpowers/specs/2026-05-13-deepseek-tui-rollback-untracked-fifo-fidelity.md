# DeepSeek-TUI Rollback Untracked FIFO Fidelity

## Context

Phase F rollback already preserved tracked staged/unstaged patches, untracked
regular files, empty directories, and Unix symlinks. `git ls-files --others`
does not report FIFO nodes, so rollback snapshots still lost generated pipes and
IPC fixture paths after a restore.

## Scope

- Capture untracked Unix FIFO paths in rollback snapshot manifests.
- Restore captured FIFOs as FIFO nodes without treating them as regular files.
- Preserve older snapshots that do not contain FIFO manifest entries.
- Include FIFO counts and paths in CLI, REPL, and TUI rollback details.
- Keep device nodes, Windows symlink recreation, and a full side-worktree
  strategy as explicit future work.

## Implementation

- `SnapshotRecord` now includes `untracked_fifos`.
- Snapshot capture scans the workspace for FIFO nodes on Unix, skipping `.git`,
  rollback storage, ignored paths, and tracked paths.
- Snapshot restore recreates FIFO nodes with `mkfifo` after removing any
  existing file or symlink at the target path.
- Snapshot JSON serialization and parsing include optional `untracked_fifos`,
  preserving backward compatibility for older manifests.
- Runtime docs and the DeepSeek-TUI parity plan now describe Unix FIFO fidelity
  and narrow the remaining Phase F special-file gap.

## Verification

- `/home/willamhou/.cargo/bin/cargo test snapshot_restore_round_trip_untracked_fifos --lib`
- `/home/willamhou/.cargo/bin/cargo test rollback --lib`
- `/home/willamhou/.cargo/bin/cargo fmt --check`
- `git diff --check`

## Remaining

Device nodes, platform-specific Windows symlink recreation, and a full
side-worktree snapshot strategy remain out of this slice. Unix socket fidelity
and untracked directory metadata are covered by follow-up rollback slices.
