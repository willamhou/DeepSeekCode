# DeepSeek-TUI Rollback Untracked Socket Fidelity

## Context

Phase F rollback preserved tracked patches, untracked regular files, empty
directories, Unix FIFOs, and Unix symlinks. Unix domain sockets were still an
explicit special-file gap: they are not reported by `git ls-files --others`,
and restore could replace a captured socket path with nothing.

## Scope

- Capture untracked Unix socket paths in rollback snapshot manifests.
- Restore captured sockets as socket filesystem nodes.
- Preserve older snapshots that do not contain socket manifest entries.
- Include socket counts and paths in CLI, REPL, and TUI rollback details.
- Keep device nodes, Windows symlink recreation, and full side-worktree
  metadata capture as future work.

## Implementation

- `SnapshotRecord` now includes `untracked_sockets`.
- Snapshot capture scans the workspace for Unix socket nodes, skipping `.git`,
  rollback storage, ignored paths, and tracked paths.
- Snapshot restore recreates socket nodes by binding a Unix listener at the
  captured path after removing any existing non-directory target.
- Snapshot JSON serialization and parsing include optional
  `untracked_sockets`, preserving backward compatibility for older manifests.
- Runtime docs and the DeepSeek-TUI parity plan narrow the remaining Phase F
  special-file gap to device nodes, Windows symlink recreation, and richer
  directory metadata. The directory metadata gap is covered by the follow-up
  rollback directory metadata slice.

## Verification

- `/home/willamhou/.cargo/bin/cargo test snapshot_restore_round_trip_untracked_sockets --lib`
- `/home/willamhou/.cargo/bin/cargo test rollback --lib`
- `/home/willamhou/.cargo/bin/cargo test restore --lib`
- `/home/willamhou/.cargo/bin/cargo test repl --lib`
- `/home/willamhou/.cargo/bin/cargo fmt --check`
- `/home/willamhou/.cargo/bin/cargo check`
- `git diff --check`

## Remaining

Device nodes, platform-specific Windows symlink recreation, and a full
side-worktree snapshot strategy remain out of this slice. Untracked directory
metadata is covered by the follow-up rollback directory metadata slice.
