# DeepSeek-TUI Rollback Empty Directory Fidelity

Date: 2026-05-13

Status: completed

## Gap

Rollback snapshots preserved tracked diffs, untracked regular files, and
untracked Unix symlinks, but empty untracked directories disappeared on restore
because Git does not report or track empty directories.

## Spec

1. Capture empty untracked workspace directories in snapshot manifests without
   including ignored paths, `.git` internals, or `.dscode/rollback` storage.
2. Restore captured empty directories on `restore_snapshot(..., apply=true)`,
   replacing a file or symlink at the same path when needed.
3. Include restored directory paths in `RestorePlan.changed_files` so CLI/TUI
   diagnostics and summaries surface the restored entries.
4. Preserve backward compatibility for older manifests that do not contain
   `untracked_directories`.
5. Update rollback CLI/TUI/REPL summaries to count files, directories, and
   symlinks as untracked entries.

## Verification

- `/home/willamhou/.cargo/bin/cargo test snapshot_restore_round_trip_empty_untracked_directories --lib`
- `/home/willamhou/.cargo/bin/cargo test snapshot_ignores_ignored_empty_directories --lib`
- `/home/willamhou/.cargo/bin/cargo test rollback --lib`
- `/home/willamhou/.cargo/bin/cargo fmt --check`
- `git diff --check`

## Implementation

- `SnapshotRecord` now includes `untracked_directories`.
- Snapshot capture walks the worktree alongside Git's untracked file list to
  find empty directories Git cannot report, while using `git check-ignore` to
  preserve existing ignored-path semantics.
- Snapshot restore creates captured directories before restoring untracked files
  and symlinks.
- Runtime docs and the DeepSeek-TUI parity plan now list empty-directory
  rollback fidelity as landed while keeping other special files as future work.
