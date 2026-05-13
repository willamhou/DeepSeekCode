# DeepSeek-TUI Rollback Directory Metadata Fidelity

Date: 2026-05-13

Status: completed

## Gap

Rollback snapshots already preserved tracked staged/unstaged patches,
untracked regular files, empty directories, Unix FIFOs, Unix sockets, and Unix
symlinks. The remaining non-privileged fidelity gap was directory metadata for
untracked directory trees: a restored tree recreated directory contents, but
lost captured Unix permission modes.

## Spec

1. Capture Unix mode metadata for directories that belong to captured
   untracked directory trees.
2. Do not change the existing `untracked_directories` meaning: it still lists
   empty directories that must be recreated.
3. Skip directories with tracked descendants so restore does not rewrite
   metadata for ordinary tracked source directories that merely contain one
   untracked file.
4. Preserve backward compatibility for older snapshots that lack the new
   manifest field.
5. Show the metadata count and paths in CLI/TUI rollback detail surfaces.

## Implementation

- Snapshot manifests now include optional `untracked_directory_metadata`
  entries with `path` and Unix `mode`.
- Restore creates missing directories when needed and applies captured modes
  deepest-first after files and special entries are restored, so restrictive
  parent modes do not block content recreation.
- Existing captured directories are temporarily made owner-readable/writable/
  searchable during restore, then returned to their captured modes.
- CLI `restore show` and the TUI rollback detail panel render directory
  metadata entries as `path mode=<octal>`.

## Verification

- `/home/willamhou/.cargo/bin/cargo test snapshot_restore_round_trip_untracked_directory_metadata --lib`
- `/home/willamhou/.cargo/bin/cargo test rollback --lib`
- `/home/willamhou/.cargo/bin/cargo test restore --lib`
- `/home/willamhou/.cargo/bin/cargo fmt --check`
- `/home/willamhou/.cargo/bin/cargo check`
- `git diff --check`
