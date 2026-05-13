# DeepSeek-TUI Rollback Untracked Symlink Fidelity

## Context

DeepSeekCode rollback snapshots already preserved tracked staged/unstaged
patches and untracked regular files. The remaining Phase F rollback gap called
out richer non-regular-file fidelity. A full side worktree strategy is larger,
but untracked symlinks are a small, useful slice because they often appear in
toolchains, generated fixtures, and local project wiring.

## Scope

- Capture untracked Unix symlinks in rollback snapshot manifests.
- Restore captured symlinks without dereferencing their targets.
- Preserve older snapshots that do not contain symlink manifest entries.
- Keep existing untracked regular file capture and restore behavior.
- Leave directory trees and other special files as explicit future work.

## Implementation

- `SnapshotRecord` now includes `untracked_symlinks`.
- Snapshot capture records each untracked symlink path and target string on
  Unix, while continuing to copy untracked regular files under `untracked/`.
- Snapshot restore recreates symlinks after removing any existing file/symlink
  at that target path, and reports restored symlink paths in `changed_files`.
- Snapshot JSON serialization and parsing include optional
  `untracked_symlinks`, preserving backward compatibility for older manifests.
- Runtime docs and the DeepSeek-TUI parity plan now describe regular-file plus
  Unix-symlink fidelity and keep broader directory/special-file support as
  remaining work.

## Verification

- `cargo test rollback --lib`
- `cargo fmt --check`
- `git diff --check`

## Remaining

Non-empty directory trees, device nodes, sockets, fifos, platform-specific
Windows symlink recreation, and a full side-worktree capture strategy remain out
of this slice.
